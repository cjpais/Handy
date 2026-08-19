# NixOS module for Handy speech-to-text
#
# Handles system-level configuration that the package wrapper cannot:
#   - udev rule for /dev/uinput (rdev grab() needs it for virtual input)
#
# Note: users must add themselves to the "input" group for evdev hotkey access.
#
# Usage in your flake:
#
#   inputs.handy.url = "github:cjpais/Handy";
#
#   nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
#     modules = [
#       handy.nixosModules.default
#       { programs.handy.enable = true; }
#     ];
#   };
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.handy;
in
{
  options.programs.handy = {
    enable = lib.mkEnableOption "Handy offline speech-to-text";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "handy.packages.\${system}.handy";
      description = "The Handy package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    # Pull pre-built binaries from the Handy Cachix cache (populated by CI in
    # .github/workflows/nix-check.yml) so users skip the ~25 min local build.
    # `extra-*` options append to the system defaults instead of replacing them.
    nix.extraOptions = ''
      extra-substituters = https://handy-computer.cachix.org
      extra-trusted-public-keys = handy-computer.cachix.org-1:Sihzctn6DC0CJM5QeL+9nBEL3CL8c33m777C+eIv748=
    '';

    # rdev grab() creates virtual input devices via /dev/uinput.
    # Default permissions are crw------- root root — open it to the input group.
    services.udev.extraRules = ''
      KERNEL=="uinput", GROUP="input", MODE="0660"
    '';
  };
}
