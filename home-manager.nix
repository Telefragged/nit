# The home-manager module: `programs.nit` puts nit on PATH and installs
# its Claude Code plugin, through `programs.claude-code` when that module
# is enabled. `services.nit` runs the review server as a systemd user
# service.
self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.nit;
in
{
  options.programs.nit = {
    enable = lib.mkEnableOption "nit, commit-level code review for AI coding agents";
    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "nit.packages.\${system}.default";
      description = "The nit package that the CLI and the server run.";
    };
  };

  options.services.nit.enable = lib.mkEnableOption "the nit review server";

  config = lib.mkMerge [
    (lib.mkIf cfg.enable { home.packages = [ cfg.package ]; })
    (lib.mkIf (cfg.enable && config.programs.claude-code.enable) {
      programs.claude-code.plugins.nit = self.packages.${pkgs.stdenv.hostPlatform.system}.claude-plugin;
    })
    (lib.mkIf (cfg.enable && !config.programs.claude-code.enable) {
      # Every switch runs this, so the plugin follows the package. The
      # activation PATH contains neither place that Claude Code installs to.
      home.activation.nitClaudePlugin = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
        (
          PATH="${config.home.profileDirectory}/bin:$HOME/.local/bin:$PATH"
          run ${lib.getExe' cfg.package "nit"} install-plugin
        )
      '';
    })
    # A new package changes the unit, and the switch restarts a changed
    # unit.
    (lib.mkIf config.services.nit.enable {
      systemd.user.services.nit = {
        Unit.Description = "nit review server";
        Service = {
          ExecStart = "${lib.getExe' cfg.package "nit"} serve";
          Restart = "on-failure";
        };
        Install.WantedBy = [ "default.target" ];
      };
    })
  ];
}
