"""Copy the standalone source distribution into an empty directory: python stage.py DEST.

Build the result with `cd DEST && maturin build`. No chartgen source or assets are copied.
"""
from pathlib import Path
import shutil
import sys

root = Path(__file__).resolve().parents[2]
dest = Path(sys.argv[1]).resolve()
dest.mkdir(parents=True, exist_ok=False)
for source in sorted((root / 'crates').iterdir()):
    if source.is_dir() and (source.name == 'figkit' or source.name.startswith('figkit-')):
        shutil.copytree(source, dest / 'crates' / source.name)
shutil.copytree(root / 'python' / 'figkit', dest / 'python' / 'figkit', ignore=shutil.ignore_patterns('__pycache__', '*.so'))
for name in ('figkit-web', 'figkit-typst', 'figkit-latex'):
    shutil.copytree(root / 'packages' / name, dest / 'packages' / name)
(dest / 'docs').mkdir()
shutil.copy(root / 'docs/figkit-guidance.md', dest / 'docs/figkit-guidance.md')
shutil.copy(root / 'packaging/figkit/README.md', dest / 'README.md')
shutil.copy(root / 'packaging/figkit/pyproject.toml', dest / 'pyproject.toml')
(dest / 'Cargo.toml').write_text('''[workspace]
resolver = "2"
members = ["crates/*"]
[workspace.package]
version = "0.1.0"
edition = "2024"
[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["arbitrary_precision", "preserve_order"] }
thiserror = "2"
''')
