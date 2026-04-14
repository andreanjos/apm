class Apm < Formula
  desc "Manage macOS AU and VST3 audio plugins from the command-line"
  homepage "https://github.com/andreanjos/apm"
  url "https://github.com/andreanjos/apm/releases/download/v0.1.1/apm-0.1.1-macos-universal.tar.gz"
  sha256 "33039a0e38f9a036c2094ebd21aacec022b26284399077795fd3a70d085e204a"
  license "MIT"

  def install
    bin.install "apm"
  end

  test do
    assert_match "apm", shell_output("#{bin}/apm --version")
  end
end
