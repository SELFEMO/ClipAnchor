cask "clipanchor" do
  version "0.7.5-rebuild"
  sha256 "9eefb5eb61070db7e245bb5e14b846385f2fc9e1ccc5eb8d103e1082e0308c40"

  url "https://github.com/SELFEMO/ClipAnchor/releases/download/pre-release-v#{version}/ClipAnchor_macOS_arm64.dmg"
  name "ClipAnchor"
  desc "Portable clipboard pinning tool"
  homepage "https://selfemo.github.io/ClipAnchor/"

  depends_on arch: :arm64
  depends_on :macos

  app "ClipAnchor.app"

  zap trash: [
    "~/Library/Application Support/ClipAnchor",
    "~/Library/LaunchAgents/com.clipanchor.desktop.plist",
  ]
end
