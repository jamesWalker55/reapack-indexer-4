# ReaPack indexer 4

An opinionated indexer for Reapack, in that it does not rely on your Git history or comment headers.

This expects a strict folder structure as input:

```plain
my-first-package/
    0.0.1/
        My First Script.lua
        version.toml
    0.0.2/
        My First Script.lua
        version.toml
        CHANGELOG.md
    package.toml
    README.md
my-other-cool-package/
    0.0.1/
        My Other Script.lua
        version.toml
    0.0.2/
        My Other Script.lua
        version.toml
        CHANGELOG.md
    package.toml
    README.md
README.md
repository.toml
```

_(^ All `*.md` readme/changelog files are optional)_

The top level contains `repository.toml`, and a folder for each package.

Each package contains `package.toml`, and a folder for each package version.

Each version contains `version.toml`, and the actual files to be distributed (all files in this folder will be included in the repository).

## Usage

```plain
> reapack-indexer-4 -h
Generate a Reapack index

Usage: reapack-indexer-4 <COMMAND>

Commands:
  export    Generate a ReaPack XML index file
  publish   Add a new version of a package, by copying the given folder to the repository
  init      Create a new repository
  template  Show a configuration file template
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

The basic flow for using this tool is like this:

```shell
mkdir my-repository
cd my-repository

reapack-indexer-4 init .
# Created repository at D:/my-repository
# Please edit the repository configuration: D:/my-repository/repository.toml

# (edit the config file)
nano repository.toml

reapack-indexer-4 publish --repo . --identifier my-first-package "D:/Programming/Files to distribute" --new
# Created package my-first-package
# Please edit the package configuration: D:/my-repository/my-first-package/package.toml
# Created version 0.0.1

reapack-indexer-4 export index.xml

cat index.xml
```

The contents of `index.xml`:

```xml
<?xml version="1.1" encoding="UTF-8"?>
<index version="1" name="my-repository">
	<category name="Category">
		<reapack desc="my-first-package" type="script" name="my-first-package">
			<version name="0.0.1" author="Your Name" time="2024-07-12T13:20:22.214444900+00:00">
				<source file="../my-first-package/0.0.1/My Cool Script.lua" main="main">https://raw.githubusercontent.com/jamesWalker55/reaper-scripting-5-index/a9e6bed9dd02148514b6e6ebf407c7740e2e3375/my-first-package/0.0.1/My Cool Script.lua</source>
				<source file="../my-first-package/0.0.1/version.toml">https://raw.githubusercontent.com/jamesWalker55/reaper-scripting-5-index/a9e6bed9dd02148514b6e6ebf407c7740e2e3375/my-first-package/0.0.1/version.toml</source>
			</version>
		</reapack>
	</category>
</index>
```
