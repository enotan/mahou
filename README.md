# mahou

mahou is a small source-based package manager I made for fun to use on my Linux From Scratch system.

It reads TOML package recipes, resolves dependencies, downloads source archives, verifies checksums, builds, installs and then records installs.

It also sync to the mahou-recipes repo, and upgrades packages if it detects a change in upstream version.

## Status

mahou is experimental, not very practical, and was made by me to learn. I doubt you were considering it, but I wouldn't recommend using it.

Working features:

- search package recipes
- show package info
- resolve dependencies
- fetch and verify source archives
- extract and build packages 
- install staged files into root
- preserve symlinks
- record installed files
- list installed packages
- sync recipe repo
- upgrade installed packages

To-do list:

- uninstall
- rollback support
- add more package recipes ofc
- add comments everywhere (im an idiot)

## Build

```sh
cargo build --release

install -Dm755 target/release/mahou /usr/bin/mahou
```

## Recipe Repository

mahou uses a seperate recipe repository, which can be found [here](https://github.com/enotan/mahou-recipes)

## Basic Usage

Search:

```sh
mahou search <name>
```

Show info:

```sh
mahou info <name>
```

Resolve dependencies:

```sh
mahou resolve <name>
```

Install:

```sh
mahou install <name>
```

Upgrade installed packages:

```sh
mahou upgrade
```

List installed packages:

```sh
mahou list
```

Show files installed by a package:

```sh
mahou files <name>
```

## Filesystem layout

mahou uses:

/var/cache/mahou/distfiles: downloaded archives
/var/cache/mahou/build: extracted source trees
/var/cache/mahou/stage: staged package installs
/var/lib/mahou/installed: installed package records
/var/lib/mahou/repos/main: synced recipe repository

## Notes on AI Usage

I did not use AI to write any of the package manager itself. I did use AI to add packages though.