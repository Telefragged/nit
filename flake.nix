{
  description = "nit — commit-level code review for AI coding agents";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            # Rust backend
            rustc
            cargo
            clippy
            rustfmt
            rust-analyzer
            pkg-config
            libgit2
            sqlite
            zlib

            # Web frontend
            nodejs_22

            # Formatting — treefmt drives the per-language formatters
            # configured in treefmt.toml (rustfmt above covers Rust)
            treefmt
            nixfmt
            prettier
            shfmt
            taplo

            # Screenshot harness / frontend checking
            playwright-driver
          ];

          env = {
            PLAYWRIGHT_BROWSERS_PATH = pkgs.playwright-driver.browsers;
            PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
            # Pin for package.json: npm playwright must match the driver.
            PLAYWRIGHT_DRIVER_VERSION = pkgs.playwright-driver.version;
          };
        };
      });

      packages = forAllSystems (pkgs: rec {
        nit-web = pkgs.buildNpmPackage {
          pname = "nit-web";
          version = "0.1.0";
          src = ./web;
          npmDepsHash = "sha256-vxdQfrgE+5vmitVuv+JJ+Ux5aVmJqDf6tjsW9AdROlU=";
          installPhase = ''
            runHook preInstall
            cp -r dist $out
            runHook postInstall
          '';
        };

        nit-unwrapped = pkgs.rustPlatform.buildRustPackage {
          pname = "nit";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [
            pkgs.libgit2
            pkgs.sqlite
            pkgs.zlib
          ];
          # The test suite builds real repos and runs `git rebase` in the
          # differential test; the sandbox has no git or identity config.
          nativeCheckInputs = [ pkgs.git ];
          preCheck = ''
            export HOME=$TMPDIR
            export GIT_AUTHOR_NAME=nix GIT_AUTHOR_EMAIL=nix@build
            export GIT_COMMITTER_NAME=nix GIT_COMMITTER_EMAIL=nix@build
          '';
        };

        # The real product: nit with the built web UI baked in via env.
        nit =
          pkgs.runCommand "nit"
            {
              nativeBuildInputs = [ pkgs.makeWrapper ];
            }
            ''
              mkdir -p $out/bin
              makeWrapper ${nit-unwrapped}/bin/nit $out/bin/nit \
                --set-default NIT_WEB_DIST ${nit-web}
            '';

        default = nit;
      });

      checks = forAllSystems (pkgs: {
        build = self.packages.${pkgs.system}.nit;
      });

      # `nix fmt` = the same whole-tree treefmt the devShell runs,
      # self-contained (formatters on PATH without entering the shell).
      formatter = forAllSystems (
        pkgs:
        pkgs.writeShellApplication {
          name = "treefmt";
          runtimeInputs = with pkgs; [
            treefmt
            rustfmt
            nixfmt
            prettier
            shfmt
            taplo
          ];
          text = ''exec treefmt "$@"'';
        }
      );
    };
}
