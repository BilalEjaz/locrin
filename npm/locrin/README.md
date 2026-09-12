# locrin

Locrin is a deterministic quality gate for code written by people and agents. It scans a repository against a fixed rule set and exits 1 when a change would ship a blocking problem, 0 when it passes, and 2 when the engine itself fails.

```sh
npm i -D locrin
npx locrin init
```

Supported platforms: Linux x64, macOS x64, macOS arm64, and Windows x64.
Each platform ships as an optional dependency that carries the prebuilt binary, so installing this package pulls only the one binary your machine needs.

Everything else, including the rules, the configuration file, and the CI setup, is in the repository README:

https://github.com/BilalEjaz/locrin
