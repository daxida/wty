use criterion::{Criterion, criterion_group, criterion_main};
use kty::cli::{ArgsOptions, DictionaryType, MainArgs, MainLangs, PathManager};
use kty::lang::{EditionLang, Lang};
use kty::{DMain, make_dict_simple};
use std::path::Path;

const BENCH_FIXTURES_DIR_100: &str = "benches/fixtures";

fn fixture_options(fixture_dir: &Path) -> ArgsOptions {
    ArgsOptions {
        save_temps: true,
        pretty: true,
        experimental: false,
        skip_yomitan: true, // !!! Skip the writing part for benching
        quiet: true,
        root_dir: fixture_dir.to_path_buf(),
        ..Default::default()
    }
}

fn fixture_main_args(
    edition: EditionLang,
    source: Lang,
    target: EditionLang,
    fixture_path: &Path,
) -> MainArgs {
    MainArgs {
        langs: MainLangs {
            edition,
            source,
            target,
        },
        options: fixture_options(fixture_path),
        ..Default::default()
    }
}

fn bench_monolingual(c: &mut Criterion, edition: EditionLang, label: &str) {
    let fixture_path = Path::new(BENCH_FIXTURES_DIR_100);
    let args = fixture_main_args(edition, edition.into(), edition, fixture_path);
    let pm = PathManager::new(DictionaryType::Main, &args);

    c.bench_function(label, |b| {
        b.iter(|| make_dict_simple(DMain, &args.options, &pm))
    });

    std::fs::remove_dir_all(pm.dir_dicts()).unwrap();
}

// cargo run -r -- main el el -r --cache-filter --skip-yomitan --first 50
fn bench_el_el(c: &mut Criterion) {
    bench_monolingual(c, EditionLang::El, "main_dict_el_el");
}

fn bench_de_de(c: &mut Criterion) {
    bench_monolingual(c, EditionLang::De, "main_dict_de_de");
}

criterion_group!(benches, bench_el_el, bench_de_de);
criterion_main!(benches);
