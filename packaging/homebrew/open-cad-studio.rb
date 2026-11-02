cask "open-cad-studio" do
  arch arm: "arm64", intel: "x86_64"

  version "2026.44"
  sha256 arm:   "fb4b1f612a0238d99cdf245f9e9c4e8677510f4cd94222c89c742a5bd8711b57",
         intel: "59c086e8a3aec13c00e01529b8f5f638eedb6a42184bc70172ff6ca7d46e3e81"

  url "https://github.com/HakanSeven12/OpenCADStudio/releases/download/v#{version}/OpenCADStudio-v#{version}-macos-#{arch}.dmg",
      verified: "github.com/HakanSeven12/OpenCADStudio/"
  name "Open CAD Studio"
  desc "CAD application for 2D drafting and 3D modeling that reads/writes DWG and DXF"
  homepage "https://github.com/HakanSeven12/OpenCADStudio"

  depends_on macos: ">= :big_sur"

  app "OpenCADStudio.app"

  # The app is ad-hoc signed but not notarised (no paid Apple Developer ID),
  # so macOS asks once on first launch: System Settings > Privacy & Security >
  # Open Anyway. Homebrew installs casks only from a tap; see README.md.

  zap trash: [
    "~/Library/Application Support/OpenCADStudio",
    "~/Library/Preferences/io.github.HakanSeven12.OpenCadStudio.plist",
    "~/Library/Saved Application State/io.github.HakanSeven12.OpenCadStudio.savedState",
  ]
end
