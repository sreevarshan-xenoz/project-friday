# friday.lock

FRIDAY OS's lock screen, a service plugin for the Omarchy shell.

- **Locking** is Omarchy's lock service (`Service.qml`, MIT, from the pinned
  Omarchy commit): the Wayland session lock, the password flow
  (`/etc/pam.d/omarchy-lock-password`), the fingerprint flow
  (`omarchy-lock-fingerprint`), stranded-lock recovery and the `lock` IPC target
  that `omarchy-system-lock`, idle and lid close call.
- **Look** (`LockView.qml`): blurred, dimmed wallpaper, a large clock and the
  date, the person's picture (`~/.face`, else their initial) in an accent ring,
  a password pill, the user name at the bottom.
- **Face unlock**: when `/etc/pam.d/friday-lock-face` exists, a face scan
  starts as soon as the screen locks, next to the password field (either one
  unlocks). A failed scan retries on activity, at most every 4 seconds; the
  face module's messages ("look at the camera", "too dark") show under the
  password.

The installer adds `omarchy.lock` to `disabledPlugins` in
`~/.config/omarchy/shell.json`, so only this plugin owns the `lock` target.
To go back to Omarchy's lock screen, remove `omarchy.lock` from that list.
