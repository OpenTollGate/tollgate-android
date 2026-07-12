#!/usr/bin/env python3
"""Post-generation patch for the UniFFI 0.28 Kotlin bindings.

UniFFI 0.28.3 generates, for an error variant whose field is named `message`,
a class with TWO `message` properties (the constructor `val message` and a
custom `override val message` getter). Kotlin 2.x rejects that as an ambiguous
overload, so the generated `tollgate_mobile.kt` does not compile.

This patch collapses the two into a single `override val message: kotlin.String`
constructor property, which cleanly overrides Throwable.message and keeps the
generated FfiConverter (which reads `value.message`) correct. It is idempotent
(running it twice is a no-op) and only touches the known-broken shape.

Run after every `just bindings`. Invoke:
    python3 scripts/patch-uniffi-bindings.py <path/to/tollgate_mobile.kt>
"""
import re
import sys

PATTERN = re.compile(
    r"class (?P<name>\w+)\(\s*\n"
    r"\s*\n"
    r"\s*val `message`: kotlin\.String\s*\n"
    r"\s*\) : TollgateException\(\) \{\s*\n"
    r"\s*override val message\s*\n"
    r'\s*get\(\) = "message=\$\{ `message` \}"\s*\n'
    r"\s*\}",
    re.MULTILINE,
)
REPL = (
    "class \g<name>(\n"
    "        \n"
    "        override val message: kotlin.String\n"
    "        ) : TollgateException()"
)


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: patch-uniffi-bindings.py <tollgate_mobile.kt>", file=sys.stderr)
        return 2
    path = sys.argv[1]
    src = open(path, encoding="utf-8").read()
    new, n = PATTERN.subn(REPL, src)
    if n == 0:
        # Already patched or pattern moved — nothing to do.
        print(f"patch-uniffi-bindings: no ambiguous `message` blocks in {path} (ok)")
        return 0
    open(path, "w", encoding="utf-8").write(new)
    print(f"patch-uniffi-bindings: fixed {n} ambiguous `message` variant(s) in {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
