// Simple javascript script to generate the download urls from the language selection.

function buildUrl(type, target, source) {
    const BASE_URL = "https://huggingface.co/datasets/daxida/test-dataset/resolve/main/dict";
    switch (type) {
        case "main":
            return `${BASE_URL}/${target}/${source}/kty-${target}-${source}.zip`;

        case "ipa":
            return `${BASE_URL}/${target}/${source}/kty-${target}-${source}-ipa.zip`;

        case "ipa-merged":
            return `${BASE_URL}/${target}/all/kty-${target}-ipa.zip`;

        case "glossary":
            return `${BASE_URL}/${target}/${source}/kty-${target}-${source}-gloss.zip`;

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

        // Glossary constraint
        if (type === "glossary" && target === source) {
            btn.disabled = true;
            info.textContent = "⚠️ Target and source must be different";
            return;
        }

        const url = buildUrl(type, target, source);
        if (!url) {
            btn.disabled = true;
            info.textContent = "";
            return;
        }

        const downloadUrl = `${url}?download=true`;

        btn.disabled = false;

        // Copy url button logic
        info.innerHTML = "";
        const copyBtn = document.createElement("button");
        copyBtn.type = "button";
        copyBtn.textContent = "📋 Copy URL";
        copyBtn.className = "copy-url-btn";

        copyBtn.onclick = async () => {
            try {
                await navigator.clipboard.writeText(downloadUrl);
                copyBtn.textContent = "✅ Copied!";
                setTimeout(() => {
                    copyBtn.textContent = "📋 Copy URL";
                }, 1500);
            } catch {
                copyBtn.textContent = "❌ Failed";
            }
        };
        info.appendChild(copyBtn);

        btn.onclick = () => {
            window.location.href = downloadUrl;
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


