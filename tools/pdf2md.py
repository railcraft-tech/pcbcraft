"""Turn datasheet.pdf in the working directory into markdown on stdout.

Images are written under images/. The first stdout line is `pages N`.
PyMuPDF 2.x exposes Document.to_markdown. PyMuPDF 1.x uses pymupdf4llm.to_markdown
from a 1.x release of that package. PyMuPDF is AGPL-3.0 or commercial and is
invoked as a separate program.
"""

import inspect
import io
import sys
from contextlib import redirect_stdout

try:
    import pymupdf
except ImportError:
    sys.stderr.write("install PyMuPDF with: pip install 'pymupdf==2.1'\n")
    sys.exit(1)

COMMON = {
    "write_images": True,
    "image_path": "images",
    "image_format": "png",
    "dpi": 150,
    "image_size_limit": 0.01,
    "force_text": True,
}
CURRENT = "pip install 'pymupdf==2.1'"
LEGACY = "pip install 'pymupdf4llm<2'"


def version_text():
    version = getattr(pymupdf, "version", None)
    if isinstance(version, (tuple, list)) and version:
        return str(version[0])
    if version:
        return str(version)
    bind = getattr(pymupdf, "VersionBind", None)
    return str(bind) if bind else "unknown"


def fail(message):
    sys.stderr.write(message)
    if not message.endswith("\n"):
        sys.stderr.write("\n")
    sys.exit(1)


def accepts_use_ocr(fn):
    """True when use_ocr is a real parameter. A bare **kwargs accepts it and
    prints a warning on stdout, which would hide the page count."""
    try:
        params = inspect.signature(fn).parameters
    except (TypeError, ValueError):
        return None
    param = params.get("use_ocr")
    if param is None:
        return False
    return param.kind != inspect.Parameter.VAR_KEYWORD


def layout_enabled():
    """Layout mode honors use_ocr. Legacy rag treats it as an unknown kwarg and
    prints that on stdout."""
    module = sys.modules.get("pymupdf4llm")
    if module is None:
        try:
            import pymupdf4llm as module
        except ImportError:
            return False
    return bool(getattr(module, "_use_layout", False))


def call(fn, *prefix):
    decision = accepts_use_ocr(fn)
    if decision is True or (decision is False and layout_enabled()):
        return fn(*prefix, use_ocr=False, **COMMON)
    if decision is False:
        return fn(*prefix, **COMMON)
    try:
        return fn(*prefix, use_ocr=False, **COMMON)
    except (TypeError, ValueError):
        return fn(*prefix, **COMMON)


def converter(doc):
    method = getattr(doc, "to_markdown", None)
    if callable(method):
        return method, ()

    try:
        import pymupdf4llm
    except ImportError:
        fail(
            f"PyMuPDF {version_text()} has no to_markdown. "
            f"For this 1.x install: {LEGACY}. "
            f"Or upgrade with: {CURRENT}"
        )

    method = getattr(doc, "to_markdown", None)
    if callable(method):
        return method, ()
    fn = getattr(pymupdf4llm, "to_markdown", None)
    if not callable(fn):
        fail(
            "pymupdf4llm has no to_markdown. "
            f"Version 2.x only works with PyMuPDF 2. For PyMuPDF {version_text()}: {LEGACY}"
        )
    return fn, (doc,)


def convert(doc):
    fn, prefix = converter(doc)
    sink = io.StringIO()
    with redirect_stdout(sink):
        text = call(fn, *prefix)
    noise = sink.getvalue()
    if noise.strip():
        sys.stderr.write(noise if noise.endswith("\n") else noise + "\n")
    return text


def main():
    try:
        doc = pymupdf.open("datasheet.pdf")
        text = convert(doc) or ""
        pages = doc.page_count
    except SystemExit:
        raise
    except Exception as err:
        fail(str(err))

    visible = []
    for line in text.splitlines():
        if "![](" in line or "](images/" in line:
            continue
        visible.append(line)
    letters = sum(1 for ch in "".join(visible) if ch.isalnum())
    if letters < 40:
        sys.stderr.write("datasheet has no text layer; figures were still saved\n")

    sys.stdout.write(f"pages {pages}\n")
    sys.stdout.write(text)
    if text and not text.endswith("\n"):
        sys.stdout.write("\n")


if __name__ == "__main__":
    main()
