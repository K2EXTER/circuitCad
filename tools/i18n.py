#!/usr/bin/env python3
"""Tools for the English table of the screen text (crates/i18n, docs/code-notes/i18n.md).

  python tools/i18n.py wrap  crates/app/src [...]   wrap Japanese text given to egui calls in tr(..) / trf!(..)
  python tools/i18n.py keys  crates [...]           every Japanese string literal (the possible keys), one per line
  python tools/i18n.py todo  OUT.tsv crates [...]   the keys with no English yet, as table lines to fill in
  python tools/i18n.py stats crates [...]           how many keys each file has and how many are translated

Only the Japanese mode is the original; `wrap` changes nothing it shows (tr/trf! give the Japanese back in that mode).
"""
import os
import re
import sys

JP = re.compile(r"[\u3040-\u30ff\u4e00-\u9fff\uff00-\uffef]")

# calls whose string arguments are shown on the screen
UI_CALLS = {
    "label", "button", "small_button", "heading", "on_hover_text", "on_disabled_hover_text", "on_hover_text_at_pointer",
    "menu_button", "checkbox", "radio", "radio_value", "selectable_label", "selectable_value", "toggle_value",
    "hyperlink_to", "hint_text", "collapsing", "weak", "strong", "monospace", "small", "colored_label", "code",
    "selected_text", "from_label", "text", "show_tooltip_text", "error_label", "prefix", "suffix",
    # small helpers of one file that draw their text argument
    "group_heading", "text_slider", "finish_kind", "field",
}
UI_NEW = {"Window", "Button", "Label", "RichText", "CollapsingHeader", "SelectableLabel", "Checkbox", "Hyperlink"}


def tokenize(src):
    """Yields (kind, start, end) for strings, chars, comments and single brackets; other text is skipped."""
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            yield ("comment", i, j)
            i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            yield ("comment", i, j)
            i = j
        elif c == "r" and re.match(r'r#*"', src[i:i + 10]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            m = re.match(r'r(#*)"', src[i:])
            close = '"' + m.group(1)
            j = src.find(close, i + len(m.group(0)))
            j = n if j < 0 else j + len(close)
            yield ("rawstr", i, j)
            i = j
        elif c == "b" and src.startswith('b"', i) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            j = i + 2
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            yield ("bytes", i, j + 1)
            i = j + 1
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            yield ("str", i, j + 1)
            i = j + 1
        elif c == "'":
            # a char literal ('a', '\n', '\u{..}') or a lifetime ('a)
            m = re.match(r"'(\\u\{[0-9a-fA-F]+\}|\\.|[^\\'])'", src[i:])
            if m:
                i += len(m.group(0))
            else:
                i += 1
        elif c in "([{":
            yield ("open", i, i + 1)
            i += 1
        elif c in ")]}":
            yield ("close", i, i + 1)
            i += 1
        else:
            i += 1


def unescape_rust(body):
    out, i = [], 0
    while i < len(body):
        c = body[i]
        if c == "\\" and i + 1 < len(body):
            d = body[i + 1]
            if d == "n":
                out.append("\n"); i += 2
            elif d == "t":
                out.append("\t"); i += 2
            elif d == "r":
                out.append("\r"); i += 2
            elif d == "0":
                out.append("\0"); i += 2
            elif d in "\\\"'":
                out.append(d); i += 2
            elif d == "u":
                m = re.match(r"\\u\{([0-9a-fA-F]+)\}", body[i:])
                out.append(chr(int(m.group(1), 16))); i += len(m.group(0))
            elif d == "\n":
                # line continuation: skip the newline and the next line's leading spaces
                i += 2
                while i < len(body) and body[i] in " \t\r\n":
                    i += 1
            else:
                out.append(c); i += 1
        else:
            out.append(c); i += 1
    return "".join(out)


def literal_value(src, kind, s, e):
    t = src[s:e]
    if kind == "rawstr":
        m = re.match(r'r(#*)"', t)
        return t[len(m.group(0)):len(t) - 1 - len(m.group(1))]
    return unescape_rust(t[1:-1])


def test_ranges(src):
    """Spans of `#[cfg(test)]` items (their braces), which are left alone."""
    spans = []
    for m in re.finditer(r"#\[cfg\(test\)\]", src):
        j = src.find("{", m.end())
        semi = src.find(";", m.end())
        if j < 0 or (0 <= semi < j):
            continue
        depth = 0
        for kind, s, e in tokenize(src[j:]):
            if kind == "open":
                depth += 1
            elif kind == "close":
                depth -= 1
                if depth == 0:
                    spans.append((m.start(), j + e))
                    break
    return spans


def skip_file(path):
    p = path.replace("\\", "/")
    return "/tests/" in p or p.endswith("tests.rs") or "/tests_" in p or p.split("/")[-1].startswith("tests_")


def rust_files(roots):
    for root in roots:
        if os.path.isfile(root):
            yield root
            continue
        for d, _, fs in os.walk(root):
            if "target" in d.split(os.sep):
                continue
            for f in sorted(fs):
                if f.endswith(".rs"):
                    yield os.path.join(d, f)


def call_name(src, paren):
    """The path before `(`: 'label', 'egui::Window::new', 'format!' ..."""
    j = paren
    while j > 0 and src[j - 1] in " \t":
        j -= 1
    m = re.search(r"([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*!?)$", src[max(0, j - 80):j])
    return m.group(1) if m else ""


def is_ui_call(name):
    if not name:
        return False
    parts = name.split("::")
    last = parts[-1]
    if last == "new" and len(parts) >= 2:
        return parts[-2] in UI_NEW
    return last in UI_CALLS


def wrap_file(path):
    src = open(path, encoding="utf-8").read()
    tests = test_ranges(src)
    in_test = lambda p: any(a <= p < b for a, b in tests)
    stack = []  # (open index, char)
    edits = []  # (start, end, replacement)
    toks = list(tokenize(src))
    for k, (kind, s, e) in enumerate(toks):
        if kind == "open":
            stack.append((s, src[s]))
        elif kind == "close":
            if stack:
                stack.pop()
        elif kind in ("str", "rawstr"):
            if in_test(s) or not JP.search(src[s:e]) or not stack:
                continue
            ps, pc = stack[-1]
            if pc == "{":
                # `if a { "x" } else { "y" }` or `match k { A => "x", .. }` given to a UI call: the literal is the whole value of
                # a block or of an arm, and the nearest paren outside the braces is the UI call
                before = src[:s].rstrip()
                after = src[e:].lstrip()
                whole_block = before.endswith("{") and after.startswith("}")
                arm = before.endswith("=>") and (after.startswith(",") or after.startswith("}"))
                if not (whole_block or arm):
                    continue
                parens = [x for x in stack if x[1] == "("]
                if not parens or len(stack) - stack.index(parens[-1]) > 3 or not is_ui_call(call_name(src, parens[-1][0])):
                    continue
                edits.append((s, s, "i18n::tr("))
                edits.append((e, e, ")"))
                continue
            if pc != "(":
                continue
            name = call_name(src, ps)
            if name == "format!":
                # format!("日本語{x}", ..) directly given to a UI call -> trf!
                if len(stack) >= 2 and stack[-2][1] == "(" and is_ui_call(call_name(src, stack[-2][0])):
                    # only the format string itself (the first token after the paren)
                    if src[ps + 1:s].strip() == "":
                        j = ps
                        while j > 0 and src[j - 1] in " \t":
                            j -= 1
                        if src[j - 7:j] == "format!" and not src[j - 9:j - 7] == "::":
                            edits.append((j - 7, j, "i18n::trf!"))
                continue
            if not is_ui_call(name):
                continue
            # the literal must be a whole argument: after '(' or ',' and before ',' or ')'
            before = src[:s].rstrip()
            after = src[e:].lstrip()
            if not (before.endswith("(") or before.endswith(",")) or not (after.startswith(",") or after.startswith(")")):
                continue
            edits.append((s, s, "i18n::tr("))
            edits.append((e, e, ")"))
    if not edits:
        return 0
    edits.sort(key=lambda x: (x[0], x[1]), reverse=True)
    out = src
    for s, e, r in edits:
        out = out[:s] + r + out[e:]
    open(path, "w", encoding="utf-8", newline="").write(out)
    return sum(1 for x in edits if x[2] != ")")


def keys(roots):
    """(file, key) for every Japanese string literal outside tests and comments."""
    for f in rust_files(roots):
        if skip_file(f):
            continue
        src = open(f, encoding="utf-8").read()
        tests = test_ranges(src)
        for kind, s, e in tokenize(src):
            if kind in ("str", "rawstr") and JP.search(src[s:e]) and not any(a <= s < b for a, b in tests):
                yield f, literal_value(src, kind, s, e)


def escape_tsv(s):
    return s.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n").replace("\r", "")


def unescape_tsv(s):
    out, i = [], 0
    while i < len(s):
        if s[i] == "\\" and i + 1 < len(s) and s[i + 1] in "nt\\":
            out.append({"n": "\n", "t": "\t", "\\": "\\"}[s[i + 1]]); i += 2
        else:
            out.append(s[i]); i += 1
    return "".join(out)


def translated():
    d = os.path.join(os.path.dirname(__file__), "..", "crates", "i18n", "en")
    done = {}
    for f in sorted(os.listdir(d)):
        if not f.endswith(".tsv"):
            continue
        for line in open(os.path.join(d, f), encoding="utf-8"):
            line = line.rstrip("\n").rstrip("\r")
            if not line.strip() or line.startswith("#") or "\t" not in line:
                continue
            ja, en = line.split("\t", 1)
            if en.strip():
                done[unescape_tsv(ja)] = en
    return done


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == "wrap":
        total = 0
        for f in rust_files(args):
            if skip_file(f):
                continue
            n = wrap_file(f)
            if n:
                print(f"{n:5} {f}")
                total += n
        print(f"{total} wrapped")
    elif cmd == "keys":
        seen = set()
        for _, k in keys(args):
            if k not in seen:
                seen.add(k)
                print(escape_tsv(k))
    elif cmd == "todo":
        out, roots = args[0], args[1:]
        done = translated()
        seen, lines = set(), []
        for f, k in keys(roots):
            if k in done or k in seen:
                continue
            seen.add(k)
            lines.append(f"# {f.replace(os.sep, '/')}\n{escape_tsv(k)}\t\n")
        with open(out, "w", encoding="utf-8", newline="\n") as fh:
            fh.writelines(lines)
        print(f"{len(lines)} keys to translate -> {out}")
    elif cmd == "stats":
        done = translated()
        per = {}
        for f, k in keys(args):
            a = per.setdefault(f, [set(), set()])
            a[0].add(k)
            if k in done:
                a[1].add(k)
        for f, (all_, ok) in sorted(per.items(), key=lambda x: -len(x[1][0])):
            print(f"{len(ok):5}/{len(all_):5}  {f}")
        print(f"total {sum(len(v[1]) for v in per.values())}/{sum(len(v[0]) for v in per.values())}")
    else:
        print(__doc__)


if __name__ == "__main__":
    main()
