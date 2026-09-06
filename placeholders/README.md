# Name-holding placeholders

Publish once each, after logging in, from this directory:

npm:     cd npm    && npm publish --access public
crates:  cd crates && cargo publish            (needs rustup installed and `cargo login`)
PyPI:    cd pypi   && python -m pip install build twine && python -m build && python -m twine upload dist/*

Bump the version before any re-publish; registries refuse re-used versions.
