# Homebrew formula for the Buff language compiler (buff-lang-cli binary).
#
# Belongs in a Homebrew tap repo (e.g. buff-lang/homebrew-tap):
#     brew tap buff-lang/tap https://github.com/buff-lang/homebrew-tap
#     brew install buff
#
# This formula downloads a PREBUILT binary (no compile-from-source). On each
# release, bump `version` and run:
#     ./installers/homebrew/update-sha256.sh <version>
# to download the matching archives from GitHub Releases and fill in the
# sha256 hashes automatically.  The on_arm/on_intel + on_macos/on_linux
# blocks select the right prebuilt artifact for the host.
#
# SHA-256 hashes are filled by the release workflow (release.yml job: update-sha256).
# Do NOT compute manually — they must match the binaries built by GitHub Actions
# per-platform.  The CI installer-lint job verifies no placeholders remain before release.
# Placeholder format uses FILL-BY-RELEASE-WORKFLOW (caught by CI installer-lint).

class Buff < Formula
  desc "Buff — a high-level language that transpiles to Rust"
  homepage "https://github.com/buff-lang/buff"
  version "1.38.0"
  license "MIT OR Apache-2.0"
  head "https://github.com/buff-lang/buff.git", branch: "v1x-frameworks"

  on_macos do
    on_arm do
      url "https://github.com/buff-lang/buff/releases/download/v1.38.0/buff-v1.38.0-macos-arm64.tar.gz"
      sha256 "FILL-BY-RELEASE-WORKFLOW"
    end

    on_intel do
      url "https://github.com/buff-lang/buff/releases/download/v1.38.0/buff-v1.38.0-macos-x64.tar.gz"
      sha256 "FILL-BY-RELEASE-WORKFLOW"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/buff-lang/buff/releases/download/v1.38.0/buff-v1.38.0-linux-arm64.tar.gz"
      sha256 "FILL-BY-RELEASE-WORKFLOW"
    end

    on_intel do
      url "https://github.com/buff-lang/buff/releases/download/v1.38.0/buff-v1.38.0-linux-x64.tar.gz"
      sha256 "FILL-BY-RELEASE-WORKFLOW"
    end
  end

  # The release archive ships a prebuilt `buff` binary + README/LICENSE, so
  # there is nothing to compile — just install the binary into the keg.
  def install
    bin.install "buff"
  end

  test do
    assert_match "buff", shell_output("#{bin}/buff --help")
  end
end
