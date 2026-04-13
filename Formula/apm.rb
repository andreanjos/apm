class Apm < Formula
  desc "Audio Plugin Manager — search, install, and manage AU/VST3 plugins from the command line"
  homepage "https://github.com/andreanjos/apm"
  version "0.1.1"

  on_macos do
    url "https://github.com/andreanjos/apm/releases/download/v#{version}/apm-#{version}-macos-universal.tar.gz"
    sha256 "33039a0e38f9a036c2094ebd21aacec022b26284399077795fd3a70d085e204a"
  end

  def install
    bin.install "apm"
  end

  test do
    assert_match "apm", shell_output("#{bin}/apm --version")
  end
end
