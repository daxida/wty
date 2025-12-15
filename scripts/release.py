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
import re
import subprocess
import time
from argparse import Namespace
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path
from pprint import pprint
from typing import Any, Literal

REPO_ID_HF = "daxida/test-dataset"
"""Full url: https://huggingface.co/datasets/daxida/test-dataset"""
REPO_ID_GH = "https://github.com/daxida/kty"

BINARY_PATH = "target/release/kty"
LOG_PATH = Path("log.txt")

ANSI_ESCAPE_RE = re.compile(r"\x1B[@-_][0-?]*[ -/]*[@-~]")


def clean(line: str) -> str:
    return ANSI_ESCAPE_RE.sub("", line)


def double_check(msg: str = "") -> None:
    if msg:
        print(msg)
    if input("Proceed? [y/n] ") != "y":
        print("Exiting.")
        exit(1)


def stats(path: Path, *, endswith: str | None = None) -> tuple[int, int]:
    n_files = 0
    size_files = 0
    for f in path.rglob("*"):
        if f.is_file():
            if endswith is not None and not f.name.endswith(endswith):
                continue
            n_files += 1
            size_files += f.stat().st_size
    return n_files, size_files


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

    _, size_bytes = stats(dict_dir)
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
    subprocess.run(
        ["cargo", "build", "--release", "--quiet"],
        check=True,
    )


def binary_version() -> str:
    result = subprocess.run(
        [BINARY_PATH, "--version"],
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout.strip()


def run_cmd(
    root_dir: Path,
    cmd_name: str,
    # <source>-<target>, <source>-<source>, all, etc.
    # they are expected to be space separated
    params: str,
    args: Namespace,
    *,
    print_download_status: bool = False,
) -> tuple[int, list[str]]:
    cmd = [
        BINARY_PATH,
        cmd_name,
        *params.split(" "),
        f"--root-dir={root_dir}",
    ]
    # Return logs to guarantee some order
    logs = []

    if args.dry_run:
        line = " ".join(cmd)
        logs.append(clean(line))
        return 0, logs

    try:
        result = subprocess.run(
            cmd,
            capture_output=True,
            check=True,  # ignore errors atm
        )
    except subprocess.CalledProcessError as e:
        # Some pairs may not have a dump from the English edition.
        # If the language is rare, this is to be expected, but there are also languages
        # like Kurdish (ku), which have an edition but no dump from the English edition.
        #
        # Every language with a dump from the English edition can be found here:
        # https://kaikki.org/dictionary/
        #
        # We ignore the 404 that we get when requesting the dictionary
        if cmd_name in ("ipa", "main") and params.split(" ")[1] == "en":
            # print("[err]", clean(" ".join(cmd)))
            return 0, logs
        log("[err]", f"Command failed: {' '.join(cmd)}")
        log("[err-stdout]", e.stdout)
        log("[err-stderr]", e.stderr)
        raise

    if args.verbose:
        out = result.stdout.decode("utf-8")
        for line in out.splitlines():
            line = clean(line)
            if "Wrote yomitan dict" in line:
                logs.append(line)
            if print_download_status and "ownload" in line:
                logs.append(line)

    return (result.returncode, logs)


def log(*values, **kwargs) -> None:
    """Poor man's loguru"""
    line = ""

    match values:
        case []:
            pass
        case [one]:
            line = one
        case [label, msg]:
            label = f"[{label}]"
            line = f"{label:<15} {msg}"
        case _:
            raise RuntimeError

    print(line, **kwargs)

    # Bad bad bad
    with LOG_PATH.open("a") as f:
        f.write(line + "\n")


type DictTy = Literal["main", "ipa", "ipa-merged", "glossary", "glossary-extended"]


def run_matrix(odir: Path, langs: list[Lang], args) -> None:
    start = time.perf_counter()

    n_workers = min(args.jobs, os.cpu_count() or 1)

    log("info", f"n_workers {n_workers}")
    check_previous_files("info", odir)
    log()

    # Clear logs
    with LOG_PATH.open("w") as f:
        f.write("")

    run_prelude()

    isos = [lang.iso for lang in langs]
    # A subset for testing
    # isos = [
    #     # "sq",
    #     # "arz",
    #     "ku",
    #     "el",
    #     "en",
    # ]

    with_edition = [lang.iso for lang in langs if lang.has_edition]
    # A subset for testing
    # with_edition = [
    #     # "el",
    #     "en",
    #     "ku",
    #     # "zh",
    #     # "ja",
    # ]

    matrix = [
        ["ipa", with_edition, isos],
        ["main", with_edition, isos],
        ["glossary", with_edition, isos],
        ["ipa-merged", with_edition, ["__target"]],
        # ["glossary-extended", isos, isos], # unsupported yet, since experimental
    ]

    log("ALL", f"Editions:  {' '.join(sorted(with_edition))}")
    log("ALL", f"Languages: {' '.join(sorted(isos))}")
    # dictionary_types: list[DictTy] = [
    #     # The order is relevant to prevent multiple workers downloading
    #     # "ipa",
    #     # "main",
    #     # "ipa-merged",
    # ]
    # log("ALL", f"Dictionaries: {' '.join(sorted(dictionary_types))}")
    log("ALL", f"Dictionaries: {' '.join(run[0] for run in matrix)}")
    log()

    # We first download the jsonl because otherwise each worker will not find it in disk
    # and will try to download it itself... This way, we guarantee only one download happens.
    #
    # NOTE: when testing with subsets, if ipa-merged is in the matrix we will download all editions...
    run_download(odir, with_edition, args)

    log("ALL", "Starting...")
    for dict_ty, sources, targets in matrix:
        dict_start = time.perf_counter()
        log(dict_ty, "Making dictionaries...")

        for source in sources:
            source_start = time.perf_counter()
            label = f"{source}-{dict_ty}"
            all_logs: list[str] = []

            def worker(target: str) -> tuple[int, list[str]]:
                match dict_ty:
                    case "main" | "ipa":
                        params = f"{target} {source}"
                    case "glossary":
                        # Ignore these
                        if source == target:
                            return 0, []
                        params = f"{source} {target}"
                    case "ipa-merged":
                        params = f"{source}"
                return run_cmd(odir, dict_ty, params, args)

            with ThreadPoolExecutor(max_workers=n_workers) as executor:
                for target, (_, logs) in zip(targets, executor.map(worker, targets)):
                    # log("DONE", f"{dict_ty} {source} {target}")
                    all_logs.extend(logs)

            for logline in sorted(all_logs):
                log(logline)

            elapsed = time.perf_counter() - source_start
            log(label, f"Finished dict ({elapsed:.2f}s)")

        # may not work:
        # glossary extended > gloss
        # main > nothing
        _, total_size = stats(odir, endswith=f"-{dict_ty}.zip")
        elapsed = time.perf_counter() - dict_start
        msg = f"Finished dicts ({elapsed:.2f}s, {total_size / 1024 / 1024:.2f}MB)"
        log(dict_ty, msg)

    n_dictionaries, total_size = stats(odir, endswith=".zip")
    elapsed = time.perf_counter() - start
    msg = f"Finished! ({elapsed:.2f}s, {total_size / 1024 / 1024:.2f}MB, {n_dictionaries} dicts)"
    log("ALL", msg)


def check_previous_files(label: str, path: Path) -> None:
    n_files, total_size = stats(path)
    if n_files > 0:
        log(
            label,
            f"Found previous files ({total_size / 1024 / 1024:.2f}MB, {n_files} files) @ {path}",
        )
    else:
        log(label, f"Clean directory. No previous files found @ {path}")


def run_prelude() -> None:
    log("prelude", "Building Rust binary...")
    build_binary()
    log("prelude", binary_version())
    rversion = release_version()
    log("prelude", f"dic {rversion}")
    log()


def run_download(odir: Path, with_edition: list[str], args: Namespace) -> None:
    start = time.perf_counter()

    log("dl", "Downloading editions...")
    download_path = odir / "kaikki"
    check_previous_files("dl", download_path)

    for source in with_edition:
        label = f"dl-{source}"
        params = f"{source} {source}"
        _, logs = run_cmd(odir, "download", params, args, print_download_status=True)
        for logline in logs:
            log(logline)
        log(label, "Finished download")

    _, total_size = stats(download_path)
    elapsed = time.perf_counter() - start
    msg = f"Finished downloads ({elapsed:.2f}s, {total_size / 1024 / 1024:.2f}MB)\n"
    log("dl", msg)


def build_release(odir: Path, args: Namespace) -> None:
    assets_path = Path("assets")
    path_languages_json = assets_path / "languages.json"
    with path_languages_json.open() as f:
        data = json.load(f)
        langs = [load_lang(row) for row in data]

    run_matrix(odir, langs, args)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("-v", "--verbose", action="store_true")
    parser.add_argument("-n", "--dry-run", action="store_true")
    parser.add_argument("-j", "--jobs", type=int, default=8)
    args = parser.parse_args()

    odir = Path("data/release")
    odir.mkdir(exist_ok=True)

    build_release(odir, args)

    # upload_to_huggingface(odir)


if __name__ == "__main__":
    main()
