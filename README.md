# RGB Lightning Sample

RGB-enabled LN node based on [ldk-sample].

The node enables the possibility to create payment channels containing assets
issued using the RGB protocol, as well as routing RGB asset denominated
payments across multiple channels, given that they all possess the necessary
liquidity. In this way, RGB assets can be transferred with the same user
experience and security assumptions of regular Bitcoin Lightning Network
payments. This is achieved by adding to each lightning commitment transaction a
dedicated extra output containing the anchor to the RGB state transition.

The RGB functionality for now can be tested only in a regtest environment,
but an advanced user may be able to apply changes in order to use it also on
other networks. Please be careful, this software is early alpha, we do not take
any responsability for loss of funds or any other issue you may encounter.

Also note that the following RGB projects (included in this project as git
sumbodules) have been modified in order to make the creation of static
consignments (without entropy) possible. Here links to compare the applied
changes:
- [bp-core](https://github.com/RGB-Tools/bp-core/compare/v0.9...static)
- [client_side_validation](https://github.com/RGB-Tools/client_side_validation/compare/v0.9...static)
- [rgb-core](https://github.com/RGB-Tools/rgb-core/compare/v0.9.0...static)
- [rgb-node](https://github.com/RGB-Tools/rgb-node/compare/v0.9.1...static)
- [rust-rgb20](https://github.com/RGB-Tools/rust-rgb20/compare/v0.9.0...static)

But most importantly [rust-lightning] has been changed in order to support
RGB channels,
[here](https://github.com/RGB-Tools/rust-lightning/compare/v0.0.113...rgb)
a compare with `v0.0.113`, the version we applied the changes to.

## Installation

Clone the project:
```sh
git clone https://github.com/RGB-Tools/ldk-sample
```

Initialize the git submodules:
```sh
git submodule update --init
```

Build the modified RGB node docker image and the ldk-sample crate:
```sh
docker-compose build
cargo build
```

## License

Licensed under either:

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

[ldk-sample]: https://github.com/lightningdevkit/ldk-sample
[rust-lightning]: https://github.com/lightningdevkit/rust-lightning
