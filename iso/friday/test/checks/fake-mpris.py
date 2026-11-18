#!/usr/bin/env python3
"""A stand-in media player on the session bus, for the shell check.

Usage: fake-mpris.py <Playing|Paused>   (runs until killed; send SIGUSR1 to
flip between playing and paused). Needs dbus-python and PyGObject."""
import signal
import sys

import dbus
import dbus.mainloop.glib
import dbus.service
from gi.repository import GLib

dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
PROPS = "org.freedesktop.DBus.Properties"
PLAYER = "org.mpris.MediaPlayer2.Player"


class Player(dbus.service.Object):
    def __init__(self, bus, status):
        super().__init__(bus, "/org/mpris/MediaPlayer2")
        self.status = status

    def props(self):
        return {
            "PlaybackStatus": self.status,
            "Metadata": dbus.Dictionary({
                "xesam:title": "Check Song",
                "xesam:artist": dbus.Array(["Check Artist"], signature="s"),
                "mpris:trackid": dbus.ObjectPath("/check/track/1"),
            }, signature="sv"),
            "CanPlay": True, "CanPause": True, "CanControl": True,
            "CanGoNext": False, "CanGoPrevious": False, "CanSeek": False,
            "Position": dbus.Int64(0), "Rate": 1.0, "Volume": 1.0,
        }

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, name):
        return self.props()[name]

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return self.props() if iface == PLAYER else {}

    @dbus.service.method(PROPS, in_signature="ssv")
    def Set(self, iface, name, value):
        pass

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass

    @dbus.service.method(PLAYER)
    def PlayPause(self):
        self.flip()

    def flip(self):
        self.status = "Paused" if self.status == "Playing" else "Playing"
        self.PropertiesChanged(PLAYER, {"PlaybackStatus": self.status}, [])


bus = dbus.SessionBus()
name = dbus.service.BusName("org.mpris.MediaPlayer2.checkplayer", bus)
player = Player(bus, sys.argv[1] if len(sys.argv) > 1 else "Playing")
GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGUSR1, lambda: player.flip() or True)
GLib.MainLoop().run()
