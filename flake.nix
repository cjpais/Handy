{
  description = "Handy - A free, open source, and extensible speech-to-text application that works completely offline";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    {
      self,
      nixpkgs,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      # Read version from Cargo.toml
      cargoToml = builtins.fromTOML (builtins.readFile ./src-tauri/Cargo.toml);
      version = cargoToml.package.version;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          lib = pkgs.lib;

          bunDeps = pkgs.stdenv.mkDerivation {
            pname = "handy-bun-deps";
            inherit version;
            src = self;

            nativeBuildInputs = [
              pkgs.bun
              pkgs.cacert
            ];

            dontFixup = true;

            buildPhase = ''
              export HOME=$TMPDIR
              bun install --frozen-lockfile --no-progress
            '';

            installPhase = ''
              mkdir -p $out
              cp -r node_modules $out/
            '';

            outputHashAlgo = "sha256";
            outputHashMode = "recursive";
            outputHash = "sha256-+hUANv0w3qnK5d2+4JW3XMazLRDhWCbOxUXQyTGta/0=";
          };
        in
        {
          handy = pkgs.rustPlatform.buildRustPackage {
            pname = "handy";
            inherit version;
            src = self;

            cargoRoot = "src-tauri";

            cargoLock = {
              lockFile = ./src-tauri/Cargo.lock;
              outputHashes = {
                "rdev-0.5.0-2" = "sha256-0F7EaPF8Oa1nnSCAjzEAkitWVpMldL3nCp3c5DVFMe0=";
                "rodio-0.20.1" = "sha256-wq72awTvN4fXZ9qZc5KLYS9oMxtNDZ4YGxfqz8msofs=";
                "tauri-nspanel-2.1.0" = "sha256-gotQQ1DOhavdXU8lTEux0vdY880LLetk7VLvSm6/8TI=";
                "tauri-runtime-2.9.1" = "sha256-sdneSI2kfRMgTkuqQoFPgtYvkqMPSuoyrffFwOph+ZM=";
                "vad-rs-0.1.5" = "sha256-Q9Dxq31npyUPY9wwi6OxqSJrEvFvG8/n0dbyT7XNcyI=";
              };
            };

            postPatch = ''
              ${pkgs.jq}/bin/jq 'del(.build.beforeBuildCommand) | .bundle.createUpdaterArtifacts = false' \
                src-tauri/tauri.conf.json > $TMPDIR/tauri.conf.json
              cp $TMPDIR/tauri.conf.json src-tauri/tauri.conf.json

              # Point libappindicator-sys to the Nix store path
              substituteInPlace \
                $cargoDepsCopy/libappindicator-sys-*/src/lib.rs \
                --replace-fail \
                  "libayatana-appindicator3.so.1" \
                  "${pkgs.libayatana-appindicator}/lib/libayatana-appindicator3.so.1"

              # Disable cbindgen in ferrous-opencc (calls cargo metadata which fails in sandbox)
              # Upstream removed this call in v0.3.1+
              substituteInPlace $cargoDepsCopy/ferrous-opencc-0.2.3/build.rs \
                --replace-fail '.expect("Unable to generate bindings")' '.ok();'
              substituteInPlace $cargoDepsCopy/ferrous-opencc-0.2.3/build.rs \
                --replace-fail '.write_to_file("opencc.h");' '// skipped'
            '';

            preBuild = ''
              cp -r ${bunDeps}/node_modules node_modules
              chmod -R +w node_modules
              substituteInPlace node_modules/.bin/{tsc,vite} \
                --replace-fail "/usr/bin/env node" "${lib.getExe pkgs.bun}"
              export HOME=$TMPDIR
              bun run build
            '';

            # Tests require runtime resources (audio devices, model files, GPU/Vulkan)
            # not available in the Nix build sandbox
            doCheck = false;

            # The tauri hook's installPhase expects target/ in cwd, but our
            # cargoRoot puts it under src-tauri/. Override to extract the DEB.
            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cd src-tauri
              mv target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/bundle/deb/*/data/usr/* $out/
              runHook postInstall
            '';

            nativeBuildInputs = with pkgs; [
              cargo-tauri.hook
              pkg-config
              wrapGAppsHook4
              bun
              jq
              cmake
              llvmPackages.libclang
              shaderc
            ];

            buildInputs = with pkgs; [
              webkitgtk_4_1
              gtk3
              glib
              glib-networking
              libsoup_3
              alsa-lib
              onnxruntime
              libayatana-appindicator
              libevdev
              libx11
              libxtst
              gtk-layer-shell
              openssl
              vulkan-loader
              vulkan-headers
              shaderc

              # Required for WebKitGTK audio/video
              gst_all_1.gstreamer
              gst_all_1.gst-plugins-base
              gst_all_1.gst-plugins-good
              gst_all_1.gst-plugins-bad
              gst_all_1.gst-plugins-ugly
            ];

            env = {
              LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
              BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${lib.getVersion pkgs.llvmPackages.libclang}/include -isystem ${pkgs.glibc.dev}/include";
              ORT_LIB_LOCATION = "${pkgs.onnxruntime}/lib";
              OPENSSL_NO_VENDOR = "1";

              # Tell Gstreamer where to find plugins
              GST_PLUGIN_SYSTEM_PATH_1_0 = "${pkgs.lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" (
                with pkgs.gst_all_1;
                [
                  gstreamer
                  gst-plugins-base
                  gst-plugins-good
                  gst-plugins-bad
                  gst-plugins-ugly
                ]
              )}";
            };

            preFixup = ''
              gappsWrapperArgs+=(
                --set WEBKIT_DISABLE_DMABUF_RENDERER 1
                --prefix LD_LIBRARY_PATH : "${
                  lib.makeLibraryPath [
                    pkgs.vulkan-loader
                    pkgs.onnxruntime
                  ]
                }"
              )
            '';

            meta = {
              description = "A free, open source, and extensible speech-to-text application that works completely offline";
              homepage = "https://github.com/cjpais/Handy";
              license = lib.licenses.mit;
              mainProgram = "handy";
              platforms = supportedSystems;
            };
          };

          default = self.packages.${system}.handy;
        }
      );

      # Development shell for building from source
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
          };
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              # Rust
              rustc
              cargo
              rust-analyzer
              clippy
              # Frontend
              nodejs
              bun
              # Tauri CLI
              cargo-tauri
              # Native deps
              pkg-config
              openssl
              alsa-lib
              libsoup_3
              webkitgtk_4_1
              gtk3
              gtk-layer-shell
              glib
              libxtst
              libevdev
              llvmPackages.libclang
              cmake
              vulkan-headers
              vulkan-loader
              shaderc
              libappindicator
            ];

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath [ pkgs.libappindicator ]}";
            GST_PLUGIN_SYSTEM_PATH_1_0 = "${pkgs.lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" (
              with pkgs.gst_all_1;
              [
                gstreamer
                gst-plugins-base
                gst-plugins-good
                gst-plugins-bad
                gst-plugins-ugly
              ]
            )}";

            # Same as wrapGAppsHook4
            XDG_DATA_DIRS = "${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:${pkgs.hicolor-icon-theme}/share";

            shellHook = ''
              echo "Handy development environment"
              bun install
              echo "Run 'bun run tauri dev' to start"
            '';
          };
        }
      );
    };
}
