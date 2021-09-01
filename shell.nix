# Latest Nightly
let
  moz_overlay = import (builtins.fetchTarball https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz);
  nixpkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
in
  with nixpkgs;
  stdenv.mkDerivation {
  name = "rust-env";
  buildInputs = [
    # Note: to use stable, just replace `nightly` with `stable`
    latest.rustChannels.nightly.rust

    # Add some extra dependencies from `pkgs`
    pkg-config openssl
  ];

  # Set Environment Variables
  RUST_BACKTRACE = 1;
  PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
}
