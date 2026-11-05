#!/usr/bin/env bash
# shellcheck disable=SC2034

iso_name="friday"
iso_label="FRIDAY_ENG"
iso_publisher="Project FRIDAY"
iso_application="FRIDAY Engineering ISO"
iso_version="$(date --date="@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y.%m.%d)"
install_dir="arch"
buildmodes=('iso')
bootmodes=('bios.syslinux'
           'uefi.systemd-boot')
pacman_conf="pacman.conf"
airootfs_image_type="squashfs"
airootfs_image_tool_options=('-comp' 'xz' '-Xbcj' 'x86' '-b' '1M' '-Xdict-size' '1M')
bootstrap_tarball_compression=('zstd' '-c' '-T0' '--auto-threads=logical' '--long' '-19')
file_permissions=(
  ["/etc/shadow"]="0:0:400"
  ["/root"]="0:0:750"
  ["/root/.automated_script.sh"]="0:0:755"
  ["/root/.gnupg"]="0:0:700"
  ["/usr/local/bin/choose-mirror"]="0:0:755"
  ["/usr/local/bin/Installation_guide"]="0:0:755"
  ["/usr/local/bin/livecd-sound"]="0:0:755"
  ["/usr/bin/friday-os"]="0:0:755"
  ["/usr/bin/omarchy"]="0:0:755"
  ["/root/configurator"]="0:0:755"
  ["/root/installer"]="0:0:755"
)
# Omarchy (fetched by fetch-omarchy.sh) keeps its executable bits, but
# mkarchiso copies airootfs without file modes, which left every Omarchy
# command except `omarchy` non-executable on installed systems (no theme, no
# Omarchy shell: no lock screen, idle or polkit agent). Restore the bit for
# each file that is executable in the checkout. mkarchiso sources this file
# from the profile directory.
if [[ -d airootfs/usr/share/omarchy ]]; then
  while IFS= read -r -d '' _f; do
    file_permissions["/${_f#airootfs/}"]="0:0:755"
  done < <(find airootfs/usr/share/omarchy -type f -perm -u+x -print0)
  unset _f
fi
