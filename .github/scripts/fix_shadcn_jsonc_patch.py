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
    '    "baseUrl": ".",\\n    "paths": {\\n      "@/*": ["./src/*"]\\n    },\\n\\n    /* Linting */\\n    "strict": true,',
)
'''
count = text.count(old)
if count != 1:
    raise SystemExit(f"expected one JSONC block, found {count}")
path.write_text(text.replace(old, new, 1), encoding="utf-8", newline="\n")
