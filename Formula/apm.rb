class Apm < Formula
  desc "Audio Plugin Manager — search, install, and manage AU/VST3 plugins from the command line"
  homepage "https://github.com/andreanjos/apm"
  version "0.1.0"

  on_macos do
    url "https://github.com/andreanjos/apm/releases/download/v#{version}/apm-#{version}-macos-universal.tar.gz"
    # SHA256 will be filled in after the first release build
    sha256 "9e3a194a51f0eee880b1c5a2399fa295158f92cd8be66a8d11b4853df3729c64"
  end

  def install
    bin.install "apm"
  end

  test do
    assert_match "apm", shell_output("#{bin}/apm --version")
  end
end
