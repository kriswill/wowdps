# NixOS module: the same systemd *user* unit as nix/home-manager.nix, for
# users who run user units without home-manager. NixOS declares user services
# in its own option shape (unit options at the top level, `serviceConfig`
# for the [Service] section) — keep the two modules' semantics in lockstep:
# graphical-session.target as ordering, stop-propagation AND hard
# precondition, since the overlay supervisor needs a compositor to spawn
# into. The WAYLAND_DISPLAY sharp edge documented in nix/home-manager.nix
# applies here identically.
{
  config,
  lib,
  ...
}:
let
  cfg = config.services.wowdps;
in
{
  options.services.wowdps = {
    enable = lib.mkEnableOption "the wowdps combat-log daemon (systemd user service)";

    package = lib.mkOption {
      type = lib.types.package;
      description = "Package providing the `wowdps` binary (the daemon).";
    };

    guiPackage = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = null;
      description = ''
        Package providing `wowdps-gui`, put on the service PATH so the
        daemon's overlay supervisor can spawn it. Null leaves overlay
        spawning to whatever PATH the user session imported.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.wowdps = {
      description = "wowdps combat-log daemon";
      after = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      # Hard precondition, not just ordering: without a graphical session
      # there is no compositor for the overlay supervisor to spawn into.
      requisite = [ "graphical-session.target" ];
      wantedBy = [ "default.target" ];
      path = lib.optional (cfg.guiPackage != null) cfg.guiPackage;
      serviceConfig = {
        # A clean exit (`wowdps stop`) stays down by design; use
        # `systemctl --user restart wowdps` to bring it back.
        ExecStart = "${cfg.package}/bin/wowdps daemon --linger";
        Restart = "on-failure";
      };
    };
  };
}
