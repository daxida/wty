"""Run the rust binary over a matrix of languages.

- The languages are collected from languages.json
- Generated dictionaries are stored @ data/release
- Then, use huggingface_hub API to:
  - update the huggingface README
  - upload the data/release folder
"""

import argparse
import datetime
import json
import os
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path
from pprint import pprint
from typing import Any

REPO_ID_HF = "daxida/test-dataset"
"""Full url: https://huggingface.co/datasets/daxida/test-dataset"""
REPO_ID_GH = "https://github.com/daxida/kty"


def double_check(msg: str = "") -> None:
    if msg:
        print(msg)
    if input("Proceed? [y/n] ") != "y":
        print("Exiting.")
        exit(1)


def directory_size_bytes(path: Path) -> int:
    return sum(f.stat().st_size for f in path.rglob("*") if f.is_file())


def release_version() -> str:
    """The version of the release.

    Different from the crate version. This uses calver.
    """
    return datetime.datetime.now().strftime("%Y-%m-%d")


# https://huggingface.co/new-dataset
# https://huggingface.co/settings/tokens
def upload_to_huggingface(odir: Path) -> None:
    dict_dir = odir / "dict"
    if not dict_dir.exists() or not any(dict_dir.iterdir()):
        print(f"No files found in {dict_dir}")
        return

    from dotenv import load_dotenv
    from huggingface_hub import HfApi, whoami

    try:
        # Requires an ".env" file with
        # HF_TOKEN="hf_..."
        load_dotenv()
        user_info = whoami()
        print(f"✓ Successfully logged in as: {user_info['name']}")
    except Exception as e:
        print(f"✗ Login failed: {e}")
        return

    size_bytes = directory_size_bytes(dict_dir)
    size_mb = size_bytes / 1024 / 1024
    version = release_version()
    git_cmd = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=".")
    commit_sha = git_cmd.decode().strip()
    commit_sha_short = commit_sha[:7]

    kwargs = dict(
        folder_path=str(dict_dir),
        path_in_repo="dict",
        repo_id=REPO_ID_HF,
        repo_type="dataset",
        commit_message=f"[{version}] update dictionaries - {commit_sha_short}",
    )

    print()
    print(commit_sha)
    pprint(kwargs)
    print(f"{version=}")
    print()
    print(f"Upload {dict_dir} ({size_mb:.2f} MB) to {REPO_ID_HF}?")
    double_check()

    api = HfApi()

    # README and .gitignore
    readme_path = odir / "README.md"
    update_readme_local(readme_path, commit_sha, version)

    try:
        api.upload_file(
            path_or_fileobj=str(readme_path),
            path_in_repo="README.md",
            repo_id=REPO_ID_HF,
            repo_type="dataset",
            commit_message=f"Update README to version {version}",
        )
    except Exception as e:
        print(e)
        exit(1)

    try:
        api.upload_folder(**kwargs)  # type: ignore
        print(f"Upload complete @ https://huggingface.co/datasets/{REPO_ID_HF}")
    except Exception as e:
        print(e)
        exit(1)


def update_readme_local(readme_path: Path, commit_sha: str, version: str) -> None:
    """Write the README of the huggingface repo @ readme_path."""
    commit_sha_short = commit_sha[:7]
    commit_sha_link = f"{REPO_ID_GH}/commit/{commit_sha}"

    readme_content = f"""---
license: cc-by-sa-4.0
---
⚠️ **This dataset is automatically uploaded.**

For source code and issue tracking, visit the GitHub repo at [kty] ({REPO_ID_GH})

version: {version}

commit: [{commit_sha_short}]({commit_sha_link})
"""

    readme_path.write_text(readme_content, encoding="utf-8")


# duplicated from build
@dataclass
class Lang:
    iso: str
    language: str
    display_name: str
    flag: str
    # https://github.com/tatuylonen/wiktextract/tree/master/src/wiktextract/extractor
    has_edition: bool


# duplicated from build
def load_lang(item: Any) -> Lang:
    return Lang(
        item["iso"],
        item["language"],
        item["displayName"],
        item["flag"],
        item.get("hasEdition", False),
    )


def build_binary() -> None:
    print("Building Rust binary...")
    subprocess.run(
        ["cargo", "build", "--release", "--quiet"],
        check=True,
    )


def run_cmd(
    root_dir: Path,
    dict_ty: str,
    source: str,
    target: str,
    verbose: bool,
) -> int:
    cmd = [
        "target/release/kty",
        dict_ty,
        source,
        target,
        f"--root-dir={root_dir}",
    ]
    if verbose:
        print(f"Running {' '.join(cmd)}")

    result = subprocess.run(
        cmd,
        capture_output=True,
        # check=True, # ignore errors atm
    )

    if verbose:
        out = result.stdout.decode("utf-8")
        for line in out.splitlines():
            if "ownload" in line or "Wrote yomitan dict" in line:
                print(line)
        # err = result.stderr.decode("utf-8")
        # print(err)

    return result.returncode


def run_matrix(odir: Path, langs: list[Lang], verbose: bool) -> None:
    build_binary()

    max_workers = min(8, os.cpu_count() or 1)
    isos = [lang.iso for lang in langs]

    for lang in langs:
        if not lang.has_edition:
            continue
        source = lang.iso
        if source != "zh":
            continue  # only greek atm

        start = time.perf_counter()

        # We first download the jsonl because otherwise each worker will not find it in disk
        # and will try to download it itself... This way, we guarantee only one download happens.
        print(f"[{source}] Downloading...")
        run_cmd(odir, "download", isos[0], source, True)
        print(f"[{source}] Finished download")

        def worker(iso: str) -> int:
            return run_cmd(odir, "main", iso, source, verbose)

        with ThreadPoolExecutor(max_workers=max_workers) as executor:
            for target, code in zip(isos, executor.map(worker, isos)):
                if handle_returncode(source, target, code):
                    return

        # Sum sizes of all *-{source}.zip files
        total_size = sum(
            f.stat().st_size
            for f in odir.rglob("*")
            if f.is_file() and f.name.endswith(f"-{source}.zip")
        )
        elapsed = time.perf_counter() - start
        print(
            f"[{source}] Finished dictionaries "
            f"(size: {total_size / 1024 / 1024:.2f}MB, time: {elapsed:.2f}s)"
        )


def handle_returncode(source: str, target: str, code: int) -> bool:
    """Return 'True' if we should abort."""
    match code:
        case 0 | 1:
            # [1] > it didn't found any entries (and therefore there is no zip)
            pass
        case 2:
            # [2] > no edition or wrong lang etc.
            pass
        case _:
            print(f"Unknown error code {code} for {source}-{target}: aborting")
            return True
    return False


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("-v", "--verbose", action="store_true")
    args = parser.parse_args()
    verbose = args.verbose

    odir = Path("data/release")
    odir.mkdir(exist_ok=True)

    # upload_to_huggingface(odir)

    assets_path = Path("assets")
    path_languages_json = assets_path / "languages.json"
    with path_languages_json.open() as f:
        data = json.load(f)
        langs = [load_lang(row) for row in data]

    run_matrix(odir, langs, verbose)


if __name__ == "__main__":
    main()
