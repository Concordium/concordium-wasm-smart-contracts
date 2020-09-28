# Contributing

This repository's CI automatically checks formatting and common problems in rust.
Changes to any of the packages must be such that
- ```cargo clippy --all``` produces no warnings
- ```rust fmt``` makes no changes.

Everything in this repository should build with stable rust at the moment (at least version 1.44 and up), however the fmt tool must be from a nightly release since some of the configuration options are not stable. One way to run the `fmt` tool is 
```
 cargo +nightly-2019-11-13 fmt
```
(the exact version used by the CI can be found in [.gitlab-ci.yml](./.gitlab-ci.yml) file). You will need to have a recent enough nightly version installed, which can be done via
```
rustup toolchain install nightly-2019-11-13
```
or similar, using the [rustup](https://rustup.rs/) tool. See the documentation of the tool for more details.

In order to contribute you should make a merge request and not push directly to master.

# Smart Contracts

This repository contains several packages to support smart contracts on and off-chain.

Currently it consists of the following parts
- [rust-contracts](./rust-contracts) which is the collection of base libraries and example smart contracts written in Rust.
- [wasmer-interp](./wasmer-interp) which is a wrapper around the [wasmer](https://github.com/wasmerio/wasmer) interpreter providing the functionality needed by the scheduler to execute smart contracts.
- [wasmer-runner](./wasmer-runner) which is a small tool that uses the API exposed in wasmer-interp to execute smart contracts directly. It can initialize and update smart contracts, in a desired state. See the `--help` option of the tool for details on how to invoke it.
- [contracts-common](./contracts-common) which contains common functionality used by smart contracts as well as the host environment to provide data for smart contracts. It defines common datatypes that need to cross boundaries, and common serialization formats.

## Rust-contracts

The [rust-contracts](./rust-contracts) aims to be organized into two (conceptually, technically three) parts.

The first consisting of crates [concordium-sc-base](./rust-contracts/concordium-sc-base) and [concordium-sc-derive](./rust-contracts/concordium-sc-derive) contains Rust packages that are meant to be developed into the core API all Rust smart contracts use. It wraps the primitives that are allowed to be used on the chain in safer wrappers. The goal is to provide an API that spans from low-level, requiring the user to be very careful, but allowing precise control over resources, to a high-level one with more safety, but less efficiency for more advanced uses.

The `concordium-sc-base` library is what is intended to be used directly, and the `concordium-sc-derive` provides some procedural macros that are re-exported by `concordium-sc-base`. These are used to remove the boilerplate FFI wrappers that are needed for each smart contract. 
Currently there are two macros `init` and `receive` that can be used to generate low-level init and receive functions.
The reason these macros are in a separate crate is because such macros must be in a special crate type `proc-macro`, which cannot have other exports than said macros.

The second, [example-contracts](./rust-contracts/example-contracts) is meant for, well, example contracts using the aforementioned API.
The list of currently implemented contracts is as follows:
- [counter](./rust-contracts/example-contracts/counter) a counter contract with a simple logic on who can increment the counter. This is the minimal example.
- [fib](./rust-contracts/example-contracts/fib) a contract calculating the requested fibonacci number, either directly or with recursive contract invocations; this is useful to demonstrate cost accounting.
- [simple-game](./rust-contracts/example-contracts/simple-game) a more complex smart contract which allows users to submit strings that are then hashed, and
  the lowest one wins after the game is over (which is determined by timeout).
  This contract uses
  - sending tokens to accounts
  - bringing in complex dependencies (containers, sha2, hex encoding)
  - more complex state, that is only partially updated.
- [escrow](./rust-contracts/example-contracts/escrow) a toy escrow contract which allows a buyer to submit a deposit which is held until the buyer is satisfied that they have received their goods, or an arbiter makes a judgement as a result of either the buyer or seller raising a dispute.
- [lockup](./rust-contracts/example-contracts/lockup) a contract which implements a GTU lockup, where those GTU vest over a pre-determined schedule, and vested GTU can be withdrawn by any one of potentially several account holders. The contract also allows for a set of accounts to have the power to veto the vesting of future GTU, e.g. for cases where an employee's vesting schedule is contingent on their continued employment.
- [erc20](./rust-contracts/example-contracts/erc20) an implementation of the [token standard](https://github.com/ethereum/EIPs/blob/master/EIPS/eip-20.md) popular in Ethereum used by other applications, such as wallets.


## Compiling smart contracts to Wasm

The process for compiling smart contracts to Wasm is always the same, and we
illustrate it here on the [counter](./rust-contracts/example-contracts/counter)
contract. To compile Rust to Wasm you need to

- install the rust wasm toolchain, for example by using
```
rustup target add wasm32-unknown-unknown
```
- run `cargo build` as
```
cargo build --target wasm32-unknown-unknown [--release]
```
(the `release` flag) is optional, by default it will build in debug builds,
which are slower and bigger.

Running `cargo build` will produce a single `.wasm` module in
`target/wasm32-unknown-unknown/release/counter.wasm` or 
`target/wasm32-unknown-unknown/debug/counter.wasm`, depending on whether the
`--release` option was used or not.

By default the module will be quite big in size, depending on the options used
(e.g., whether it is compiled with `std` or not, it can be from 600+kB to more
than a MB). However most of that code is redundant and can be stripped away.
There are various tools and libraries for this. One such suite of tools is [Web
assembly binary toolkit (wabt)](https://github.com/WebAssembly/wabt) and its
tool `wasm-strip`.

Using `wasm-strip` on the produced module produces a module of size 11-13kB ,
depending on whether the `no_std` option was selected or not.

### Default toolchain

The default toolchain can be specified in the `.cargo/config` files inside the
project, as exemplified in the
[counter/.cargo/config](./rust-contracts/example-contracts/counter/.cargo/config)
file.

### Compilation options

Since a contract running on the chain will typically not be able to recover from
panics, and error traces are not reported, it is useful not to bloat code size
with them. Setting `panic=abort` will make it so that the compiler will generate
simple `Wasm` traps on any panic that occurs. This option can be specified
either in `.cargo/config` as exemplified in
[counter/.cargo/config](./rust-contracts/example-contracts/counter/.cargo/config), 
or in the `Cargo.toml` file as

```
[profile.release]
# Don't unwind on panics, just trap.
panic = "abort"
```

The latter will only set this option in `release` builds, for debug builds use

```
[profile.dev]
# Don't unwind on panics, just trap.
panic = "abort"
```
instead.

An additional option that might be useful to minimize code size at the cost of
some performance in some cases is
```
[profile.release]
# Tell `rustc` to optimize for small code size.
opt-level = "s"
```
or even `opt-level = "z"`.

In some cases using `opt-level=3` actually leads to smaller code sizes, presumably due to more inlining and dead code removal as a result.

# Example inputs to the wasmer-runner

The following are some example invocations of the `wasmer-runner` binary.

```shell
./wasmer-runner init --context init-context.json --parameter parameter.bin --source ./simple_game.wasm --out state.bin --amount 123
```

with input files

```json
{
    "metadata": {
        "slotNumber": 1,
        "blockHeight": 1,
        "finalizedHeight": 1,
        "slotTime": 123000
    },
    "initOrigin": "3uxeCZwa3SxbksPWHwXWxCsaPucZdzNaXsRbkztqUUYRo1MnvF"
}
```

and `parameter.bin` as

```
00001111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
```

(as a text file without a newline).

```shell
./wasmer-runner update --context receive-context.json --parameter parameter-receive.bin --source ./simple_game.wasm --state state-in.bin --amount 0 --name "receive_help_yourself" --balance 13 --out state-out.bin
```

where an example receive context is

```json
{
    "metadata": {
        "slotNumber": 1,
        "blockHeight": 1,
        "finalizedHeight": 1,
        "slotTime": 12312312312312312312
    },
    "invoker": "3uxeCZwa3SxbksPWHwXWxCsaPucZdzNaXsRbkztqUUYRo1MnvF",
    "selfAddress": {"index": 0, "subindex": 0},
    "selfBalance": 0,
    "sender": {
        "type": "Account",
        "address": "3uxeCZwa3SxbksPWHwXWxCsaPucZdzNaXsRbkztqUUYRo1MnvF"
    },
    "owner": "3uxeCZwa3SxbksPWHwXWxCsaPucZdzNaXsRbkztqUUYRo1MnvF"
}
```

See `--help` or `help` option to `wasmer-runner` for an explanation of the options.

# Testing smart contracts

Testing of smart contracts should be done at many different levels, from immediate unit testing of functionality that is used in smart contracts, through testing individual invocations of `init` and `receive` methods, to end-to-end testing in the scheduler.

The first and second can be done directly in the module the contract is written in. This relies on the contract's init and receive methods being written with a generic enough signature so that the host functions, normally provided by the scheduler, can be replaced by a test harness.

The design is as follows. 

- Each of the host-provided parameters to the init and receive methods has its own trait. 
These are defined in [concordium-sc-base/src/traits.rs](./rust-contracts/concordium-sc-base/src/traits.rs).
- The traits have implementations that are used when the contract is invoked with host functions. These are defined in [concordium-sc-base/src/impls.rs](./rust-contracts/concordium-sc-base/src/impls.rs).
- Additionally, there are implementations of these traits that allow calling of smart contracts in a way that is easy to specify parameters, run the contract, and inspect the result, all entirely inside `Rust`. These are defined in [concordium-sc-base/src/test_infrastructure.rs](./rust-contracts/concordium-sc-base/src/test_infrastructure.rs), together with the wrappers that can be used for testing.
- The intended use of this functionality is exemplified in the tests in the [counter-smart-contract](./rust-contracts/example-contracts/counter/src/lib.rs).

Currently the only way to run tests is to compile to native code. This can be done by explicitly specifying the target as 
```
cargo test --release --target=x86_64-unknown-linux-gnu
```
or similar, depending on the platform (alternatively just comment out the default target in `.cargo/config`).

This kind of testing is perfectly adequate for a large amount of functional correctness testing, however ultimately we also want to test code as it will be deployed to the chain. For this, the intention is to update the `wasmer-runner` with a `test` command that will be able to execute smart contracts in a given state and parameters.

We might hook into the default testing infrastructure of Rust by specifying a binary runner in `.cargo/config` as well for this, although the best user-experience needs to be determined.


# Removing Host Information from Binary
By default the compiled binary from a rust crate contains some information from the host machine, namely rust-related paths such as the path to `.cargo`. This can be seen by inspecting the produced binary:

Lets assume your username is `tom` and you have a smart contract `foo` located in your home folder, which you compiled in release-mode to WASM32.
By running the following command inside the `foo` folder, you will be able to see the paths included in the binary: `strings target/wasm32-unknown-unknown/release/foo.wasm | grep tom`

To remove the host information, the path prefixes can be remapped using a flag given to the compiler.
`RUSTFLAGS=--remap-path-prefix=/home/tom=secret cargo build --release --target wasm32-unknown-unknown`, where `/home/tom` is the prefix you want to change into `secret`.
The flag can be specified multiple times to remap multiple prefixes.

The flags can also be set permanently in the `.cargo/config` file in your crate, under the `build` section:

``` toml
[build]
rustflags = ["--remap-path-prefix=/home/tom=secret"]
```

**Important:**
[--remap-path-prefix does currently not work correctly if the `rust-src` component is present.](https://github.com/rust-lang/rust/issues/73167)
