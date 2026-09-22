cask "clipanchor" do
  # "<version after v>,<pre-release|release>" — the tag is "#{channel}-v#{version}" and the number after v only increases.
  version "0.7.5-rebuild,pre-release"
  sha256 "9eefb5eb61070db7e245bb5e14b846385f2fc9e1ccc5eb8d103e1082e0308c40"

  url "https://github.com/SELFEMO/ClipAnchor/releases/download/#{version.csv.second}-v#{version.csv.first}/ClipAnchor_macOS_arm64.dmg"
  name "ClipAnchor"
  desc "Portable clipboard pinning tool"
  homepage "https://selfemo.github.io/ClipAnchor/"

  depends_on arch: :arm64
  depends_on :macos

  app "ClipAnchor.app"

  postflight_steps do
    run "/usr/bin/xattr",
        args:         ["-dr", "com.apple.quarantine", "{{appdir}}/ClipAnchor.app"],
        must_succeed: false
  end

  zap trash: [
    "~/Library/Application Support/ClipAnchor",
    "~/Library/LaunchAgents/com.clipanchor.desktop.plist",
  ]
end
