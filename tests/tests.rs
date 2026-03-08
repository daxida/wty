use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Ok, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use zip::ZipArchive;

use wty::cli::{DictName, GlossaryArgs, GlossaryLangs, IpaArgs, MainArgs, MainLangs, Options};
use wty::dict::{DGlossary, DIpa, DMain};
use wty::lang::{Edition, Lang};
use wty::make_dict;
use wty::path::PathManager;

/// Clean empty folders under folder "root" recursively.
fn cleanup(root: &Path) -> bool {
    let entries = fs::read_dir(root).unwrap();

    let mut is_empty = true;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let child_empty = cleanup(&path);
            if child_empty {
                fs::remove_dir(&path).unwrap();
            } else {
                is_empty = false;
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
        {
            panic!("zip found in tests");
        } else {
            is_empty = false;
        }
    }

    is_empty
}

fn fixture_options(fixture_dir: &Path) -> Options {
    Options {
        save_temps: true,
        pretty: true,
        experimental: false,
        root_dir: fixture_dir.to_path_buf(),
        ..Default::default()
    }
}

fn output_options(root_dir: &Path, stream: bool) -> Options {
    Options {
        pretty: true,
        stream,
        root_dir: root_dir.to_path_buf(),
        ..Default::default()
    }
}

fn fixture_main_args(source: Lang, target: Edition, fixture_dir: &Path) -> MainArgs {
    MainArgs {
        langs: MainLangs {
            source: source,
            target: target,
        },
        dict_name: DictName::default(),
        options: fixture_options(fixture_dir),
    }
}

fn output_main_args(source: Lang, target: Edition, root_dir: &Path, stream: bool) -> MainArgs {
    MainArgs {
        langs: MainLangs { source, target },
        dict_name: DictName::default(),
        options: output_options(root_dir, stream),
    }
}

fn fixture_ipa_args(source: Lang, target: Edition, fixture_dir: &Path) -> IpaArgs {
    IpaArgs {
        langs: MainLangs {
            source: source,
            target: target,
        },
        dict_name: DictName::default(),
        options: fixture_options(fixture_dir),
    }
}

fn fixture_glossary_args(source: Edition, target: Lang, fixture_dir: &Path) -> GlossaryArgs {
    GlossaryArgs {
        langs: GlossaryLangs {
            source: source,
            target: target,
        },
        dict_name: DictName::default(),
        options: fixture_options(fixture_dir),
    }
}

fn setup_tracing_test() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_span_events(FmtSpan::CLOSE)
        .with_target(true)
        .with_level(true)
        .init();
}

fn temp_root(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("wty-test-{label}-{unique}"))
}

fn gzip_fixture(path: &Path) -> Vec<u8> {
    let input = fs::read(path).unwrap();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&input).unwrap();
    encoder.finish().unwrap()
}

fn start_kaikki_server(body: Vec<u8>, request_count: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        for _ in 0..request_count {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buf = [0_u8; 1024];
            loop {
                let bytes_read = stream.read(&mut buf).unwrap();
                if bytes_read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..bytes_read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let request_text = String::from_utf8(request).unwrap();
            let request_line = request_text.lines().next().unwrap();
            assert_eq!(
                request_line,
                "GET /dictionary/raw-wiktextract-data.jsonl.gz HTTP/1.1"
            );

            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
            stream.flush().unwrap();
        }
    });

    (format!("http://{addr}"), handle)
}

fn zip_contents(path: &Path) -> Vec<(String, Vec<u8>)> {
    let file = fs::File::open(path).unwrap();
    let mut zip = ZipArchive::new(file).unwrap();
    let mut contents = Vec::new();

    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).unwrap();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        contents.push((entry.name().to_string(), data));
    }

    contents.sort_by(|left, right| left.0.cmp(&right.0));
    contents
}

/// Test via snapshots and git diffs like the original
#[test]
fn snapshot() {
    setup_tracing_test();

    let fixture_dir = PathBuf::from("tests");
    // have to hardcode this since we have not initialized args
    let fixture_input_dir = fixture_dir.join("kaikki");

    // Nuke the output dir to prevent pollution
    // It has the disadvantage of massive diffs if we failfast.
    //
    // let fixture_output_dir = fixture_dir.join("dict");
    // Don't crash if there is no output dir. It may happen if we nuke it manually
    // let _ = fs::remove_dir_all(fixture_output_dir);

    let mut cases = Vec::new();
    let mut langs_in_testsuite = Vec::new();

    // iterdir and search for source-target-extract.jsonl files
    for entry in fs::read_dir(&fixture_input_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if let Some(fname) = path.file_name().and_then(|f| f.to_str())
            && let Some(base) = fname.strip_suffix("-extract.jsonl")
            && let Some((source, target)) = base.split_once('-')
        {
            let src = source.parse::<Lang>().unwrap();
            let tar = target.parse::<Lang>().unwrap();
            cases.push((src, tar));

            if !langs_in_testsuite.contains(&src) {
                langs_in_testsuite.push(src);
            }
            if !langs_in_testsuite.contains(&tar) {
                langs_in_testsuite.push(tar);
            }
        }
    }

    tracing::debug!("Found {} cases: {cases:?}", cases.len());

    // failfast
    // main
    for (source, target) in &cases {
        let Result::Ok(target) = (*target).try_into() else {
            continue; // skip if target is not edition
        };
        let args = fixture_main_args(*source, target, &fixture_dir);

        if let Err(e) = shapshot_main(args) {
            panic!("({source}): {e}");
        }
    }

    // glossary
    for (source, target) in &cases {
        if source != target {
            continue;
        }

        let Result::Ok(source) = (*source).try_into() else {
            continue; // skip if source is not edition
        };

        for possible_target in &langs_in_testsuite {
            if Lang::from(source) == *possible_target {
                continue;
            }
            let args = fixture_glossary_args(source, *possible_target, &fixture_dir);
            make_dict(DGlossary, args).unwrap();
        }
    }

    // ipa
    for (source, target) in &cases {
        let Result::Ok(target) = (*target).try_into() else {
            continue; // skip if target is not edition
        };
        let args = fixture_ipa_args(*source, target, &fixture_dir);
        make_dict(DIpa, args).unwrap();
    }

    cleanup(&fixture_dir.join("dict"));
}

#[test]
fn streamed_main_matches_cached_main_without_creating_cache_files() {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let fixture = PathBuf::from("tests/kaikki/ja-en-extract.jsonl");
    let gzipped_fixture = gzip_fixture(&fixture);
    let (base_url, server) = start_kaikki_server(gzipped_fixture, 2);

    let cached_root = temp_root("cached-main");
    let streamed_root = temp_root("streamed-main");
    fs::create_dir_all(&cached_root).unwrap();
    fs::create_dir_all(&streamed_root).unwrap();

    unsafe {
        std::env::set_var("WTY_KAIKKI_ROOT_URL", &base_url);
    }

    let cached_args = output_main_args(Lang::Ja, Edition::En, &cached_root, false);
    let streamed_args = output_main_args(Lang::Ja, Edition::En, &streamed_root, true);

    let cached_pm = PathManager::try_from(cached_args.clone()).unwrap();
    let streamed_pm = PathManager::try_from(streamed_args.clone()).unwrap();

    make_dict(DMain, cached_args).unwrap();
    make_dict(DMain, streamed_args).unwrap();

    unsafe {
        std::env::remove_var("WTY_KAIKKI_ROOT_URL");
    }

    server.join().unwrap();

    assert!(cached_pm.dir_kaik().exists());
    assert!(!streamed_pm.dir_kaik().exists());
    assert_eq!(
        zip_contents(&cached_pm.path_dict()),
        zip_contents(&streamed_pm.path_dict())
    );

    let _ = fs::remove_dir_all(cached_root);
    let _ = fs::remove_dir_all(streamed_root);
}

/// Delete generated artifacts from previous tests runs, if any
fn delete_previous_output(pm: &PathManager) -> Result<()> {
    let pathdir_dict_temp = pm.dir_temp_dict();
    if pathdir_dict_temp.exists() {
        tracing::debug!("Deleting previous output: {pathdir_dict_temp:?}");
        fs::remove_dir_all(pathdir_dict_temp)?;
    }
    Ok(())
}

/// Run git --diff for charges in the generated json
fn check_git_diff(pm: &PathManager) -> Result<()> {
    let output = std::process::Command::new("git")
        .args([
            "diff",
            "--color=always",
            "--unified=0", // show 0 context lines
            "--",
            // we don't care about changes in tidy files
            &pm.dir_temp_dict().to_string_lossy(),
        ])
        .output()?;
    if !output.stdout.is_empty() {
        eprintln!("{}", String::from_utf8_lossy(&output.stdout));
        anyhow::bail!("changes!")
    }

    Ok(())
}

/// Read the expected result in the snapshot first, then git diff
fn shapshot_main(margs: MainArgs) -> Result<()> {
    let pm = &PathManager::try_from(margs.clone())?;
    delete_previous_output(pm)?;
    make_dict(DMain, margs)?;
    check_git_diff(pm)?;
    Ok(())
}
