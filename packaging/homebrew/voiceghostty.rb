cask "voiceghostty" do
  version "0.1.0"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/10XTeams/VoiceGhostty/releases/download/v#{version}/VoiceGhostty-v#{version}.zip",
      verified: "github.com/10XTeams/VoiceGhostty/"
  name "VoiceGhostty"
  desc "Voice-first terminal for Claude Code"
  homepage "https://github.com/10XTeams/VoiceGhostty"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: :ventura   # macOS 13+, matching LSMinimumSystemVersion

  app "VoiceGhostty.app"

  # The build is ad-hoc signed, not notarized (no paid Developer ID), so Gatekeeper would refuse to
  # open it. Homebrew *adds* a quarantine flag by default — `quarantine: true` in cask/installer.rb —
  # so without this the app installs and then won't launch.
  #
  # Clearing it here is the same thing the README used to ask users to do by hand; it is not a way
  # around any check they weren't already making themselves by choosing to tap this repo.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-cr", "#{appdir}/VoiceGhostty.app"],
                   sudo: false
  end

  caveats <<~EOS
    VoiceGhostty is ad-hoc signed and NOT notarized by Apple.

    This cask cleared the quarantine flag for you after install, which is what lets the app open at
    all. If you would rather Gatekeeper stayed in charge, uninstall and build from source instead:
      https://github.com/10XTeams/VoiceGhostty#option-c--build-from-source

    On first launch macOS will ask for Microphone and Speech Recognition access.
  EOS

  zap trash: [
    "~/.config/voiceghostty",
    "~/Library/Preferences/com.10xteams.voiceghostty.plist",
    "~/Library/Saved Application State/com.10xteams.voiceghostty.savedState",
  ]
end
