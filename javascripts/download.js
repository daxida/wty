// Simple javascript script to generate the download urls from the language selection.

function buildUrl(type, target, source) {
    const BASE_URL = "https://huggingface.co/datasets/daxida/test-dataset/resolve/main/dict"
    switch (type) {
        case "main":
            return {
                url: `${BASE_URL}/${target}/${source}/kty-${target}-${source}.zip`,
                filename: `kty-${target}-${source}.zip`,
            };

        case "ipa":
            return {
                url: `${BASE_URL}/${target}/${source}/kty-${target}-${source}-ipa.zip`,
                filename: `kty-${target}-${source}-ipa.zip`,
            };

        case "ipa-merged":
            return {
                url: `${BASE_URL}/${target}/all/kty-${target}-ipa.zip`,
                filename: `kty-${target}-ipa.zip`,
            };

        case "glossary":
            return {
                url: `${BASE_URL}/${target}/${source}/kty-${target}-${source}-gloss.zip`,
                filename: `kty-${target}-${source}-gloss.zip`,
            };

        default:
            return null;
    }
}

function setupRow(row) {
    const type = row.dataset.type;
    const targetSel = row.querySelector(".dl-target");
    const sourceSel = row.querySelector(".dl-source");
    const btn = row.querySelector(".dl-btn");
    const info = row.querySelector(".dl-info");

    function update() {
        const target = targetSel?.value;
        const source = sourceSel?.value;

        // Dummies
        if (target === "" || source === "") {
            btn.disabled = true;
            info.textContent = "Select the language(s)";
            return;
        }
        console.log(target, source);

        // Glossary constraint
        if (type === "glossary" && target === source) {
            btn.disabled = true;
            info.textContent = "⚠️ Target and source must be different";
            return;
        }

        const result = buildUrl(type, target, source);
        if (!result) {
            btn.disabled = true;
            info.textContent = "";
            return;
        }

        btn.disabled = false;
        info.textContent = `File: ${result.filename}`;

        btn.onclick = () => {
            window.location.href = `${result.url}?download=true`;
        };
    }

    targetSel?.addEventListener("change", update);
    sourceSel?.addEventListener("change", update);

    update();
}

// I don't think this is ideal (it is called on every tab switch, and not only on the download's one),
// but it's the only thing I got working...
// cf. https://github.com/squidfunk/mkdocs-material/discussions/6788#discussioncomment-8498415
document$.subscribe(function() {
    document
        .querySelectorAll(".download-table tr[data-type]")
        .forEach(setupRow);
})


