class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.5.6"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.6/delphi-v0.5.6-aarch64-apple-darwin.tar.gz"
      sha256 "9f05dc9bb5197305334fd8f0dd685c2726ba9126899ba202f7ee31b05552c7ac"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.6/delphi-v0.5.6-x86_64-apple-darwin.tar.gz"
      sha256 "6876e25a32280434a9a01130ee7c5f61053d8f98094eb8921c71f7b3f707154e"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.6/delphi-v0.5.6-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "195bbc97fc5ad6b626d0ba81517435b4614e2ed89d264f1507e6cc8f3186307e"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.5.6/delphi-v0.5.6-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "840ed80b83db695ee20faa7aa8f55eff3625e09dd261e2a13a43ef539e55c945"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
