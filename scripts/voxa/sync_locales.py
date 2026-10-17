"""Inject the Voxa strings into every locale file.

Source of truth: src/voxa/i18n/en.json and ru.json. The Russian locale gets the
Russian block; every other locale gets the English one (the translation check
needs identical key sets in all locales). Idempotent: an existing "voxa" key is
replaced. Text is spliced in, not re-serialized, so the rest of each file keeps
its exact formatting.

    python3 scripts/voxa/sync_locales.py
"""

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOCALES = ROOT / "src" / "i18n" / "locales"
SRC = ROOT / "src" / "voxa" / "i18n"


def block(name: str) -> str:
    data = json.loads((SRC / f"{name}.json").read_text(encoding="utf-8"))
    body = json.dumps(data, indent=2, ensure_ascii=False)
    indented = body.replace("\n", "\n  ")
    return f'  "voxa": {indented}'


def splice(text: str, new_block: str) -> str:
    text = text.rstrip()
    assert text.endswith("}"), "locale file must end with }"
    text = text[:-1].rstrip()
    # drop an existing voxa block (always the last key when we wrote it)
    match = re.search(r',\n  "voxa": \{', text)
    if match:
        text = text[: match.start()]
    return f"{text},\n{new_block}\n}}\n"


def main() -> None:
    blocks = {"en": block("en"), "ru": block("ru")}
    for path in sorted(LOCALES.glob("*/translation.json")):
        lang = path.parent.name
        new_text = splice(path.read_text(encoding="utf-8"), blocks.get(lang, blocks["en"]))
        json.loads(new_text)  # must stay valid
        path.write_text(new_text, encoding="utf-8")
    print("voxa strings written to", len(list(LOCALES.glob("*/translation.json"))), "locales")


if __name__ == "__main__":
    main()
