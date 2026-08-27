# Home-manager module: runs the wowdps daemon as a systemd user service so
# `auto_overlay` can catch the *next* game launch — the whole point of the
# daemon being a session service.
#
#   imports = [ wowdps.homeManagerModules.default ];
#   services.wowdps.enable = true;
#
# Sharp edge (documented, deliberate): the auto-launched overlay is a Wayland
# client. It needs WAYLAND_DISPLAY/XDG_RUNTIME_DIR in the *systemd user*
# environment (`systemctl --user import-environment WAYLAND_DISPLAY` from the
# compositor session, which Hyprland's uwsm/dbus activation normally does) and
# a `wowdps-gui` that can find its runtime libraries. Spawn failures are not
# silent: they surface in `wowdps status` and the daemon log at
# `$XDG_STATE_HOME/wowdps/daemon.log`.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.wowdps;
in
{
  options.services.wowdps = {
    enable = lib.mkEnableOption "the wowdps combat-log daemon";

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
      Unit = {
        Description = "wowdps combat-log daemon";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
        # Hard precondition, not just ordering: without a graphical session
        # there is no compositor for the overlay supervisor to spawn into.
        Requisite = [ "graphical-session.target" ];
      };
      Service = {
        # A clean exit (`wowdps stop`) stays down by design; use
        # `systemctl --user restart wowdps` to bring it back.
        ExecStart = "${cfg.package}/bin/wowdps daemon --linger";
        Restart = "on-failure";
        Environment = lib.optional (cfg.guiPackage != null) "PATH=${lib.makeBinPath [ cfg.guiPackage ]}";
      };
      Install.WantedBy = [ "default.target" ];
    };
  };
}
