from pathlib import Path

path = Path(".github/scripts/shadcn_projects_patch.py")
text = path.read_text(encoding="utf-8")
old = '''# TypeScript alias used by shadcn generated components.
tsconfig = json.loads(Path("tsconfig.json").read_text(encoding="utf-8"))
compiler = tsconfig.setdefault("compilerOptions", {})
compiler["baseUrl"] = "."
compiler["paths"] = {"@/*": ["./src/*"]}
write("tsconfig.json", json.dumps(tsconfig, ensure_ascii=False, indent=2) + "\\n")
'''
new = '''# TypeScript alias used by shadcn generated components. Keep JSONC comments intact.
replace_exact(
    "tsconfig.json",
    '    /* Linting */\\n    "strict": true,',
    '    "paths": {\\n      "@/*": ["./src/*"]\\n    },\\n\\n    /* Linting */\\n    "strict": true,',
)
'''
count = text.count(old)
if count != 1:
    raise SystemExit(f"expected one JSONC block, found {count}")
text = text.replace(old, new, 1)

old_vite = 'path.resolve(__dirname, "./src")'
new_vite = 'path.resolve(import.meta.dirname, "./src")'
if text.count(old_vite) != 1:
    raise SystemExit(f"expected one Vite dirname marker, found {text.count(old_vite)}")
text = text.replace(old_vite, new_vite, 1)

path.write_text(text, encoding="utf-8", newline="\n")
