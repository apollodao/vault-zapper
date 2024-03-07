# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2024-03-07

### Changed

- Bumped `cw-dex` to `0.5.3`
- Use crates `cw-dex-astroport` and `cw-dex-osmosis` instead of the now deprecated features of `cw-dex`. This means moving the `Pool` enum to this crate.
- Bump `cw2` to `1.1.2`
- Bump `cw-it` to `0.3.1`
- Bump `locked-astroport-vault` and `locked-astroport-vault-test-helpers` to `0.4.2`.
- Bump `cw-vault-standard-test-helpers` to `0.4.1`.

### Added

- Added check that either `astroport` or `osmosis` features, or both, are enabled.

## [0.2.0] - 2023-11-06

### Changed

- Bumped `cw-dex` to `0.5.0`
  - This required adding the field `astroport_liquidity_manager: String` to `InstantiateMsg`.
- Bumped `cw-dex-router` to `0.3.0`
- Bumped `liquidity-helper` and `astroport-liquidity-helper` to `0.3.0`
- Bumped `cosmwasm-std` and `cosmwasm-schema` to `1.5.0`
- Bumped `cw-vault-standard` to `0.4.0`
