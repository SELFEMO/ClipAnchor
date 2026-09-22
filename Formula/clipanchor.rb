class Clipanchor < Formula
  desc "Portable clipboard pinning tool"
  homepage "https://selfemo.github.io/ClipAnchor/"
  url "https://github.com/SELFEMO/ClipAnchor/releases/download/pre-release-v0.7.5-rebuild/ClipAnchor_Linux_x64.deb"
  version "0.7.5-rebuild"
  sha256 "52fda3c3223e9ab69817b367b933b52f04ed70fb7c63e8975e1cfb0fb80359b4"

  depends_on arch: :x86_64
  depends_on :linux

  def install
    system "ar", "x", cached_download
    system "tar", "xf", "data.tar.gz"

    libexec.install "usr/bin/clipanchor"
    share.install "usr/share/icons"

    (share/"applications/ClipAnchor.desktop").write <<~EOS
      [Desktop Entry]
      Categories=Utility;
      Comment=Portable clipboard pinning tool
      Exec=#{opt_bin}/clipanchor
      StartupWMClass=clipanchor
      Icon=clipanchor
      Name=ClipAnchor
      Terminal=false
      Type=Application
    EOS

    (bin/"clipanchor").write <<~SH
      #!/bin/bash
      export CLIPANCHOR_DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/ClipAnchor/data"
      exec "#{libexec}/clipanchor" "$@"
    SH
    chmod 0755, bin/"clipanchor"
  end

  def caveats
    <<~EOS
      ClipAnchor is linked against the distribution's GTK and WebKitGTK libraries.
      Install webkit2gtk-4.1, gtk-3, and libayatana-appindicator3 (or the equivalent packages) before launching.
      Runtime data is stored in ${XDG_DATA_HOME:-$HOME/.local/share}/ClipAnchor/data.
    EOS
  end
end
