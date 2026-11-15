class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.6.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.2/delphi-v0.6.2-aarch64-apple-darwin.tar.gz"
      sha256 "9879116b4623dab592cd00bf0be4df62ba8b21160b6a6a720064f484ab432165"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.2/delphi-v0.6.2-x86_64-apple-darwin.tar.gz"
      sha256 "da18f7310e33ad688c174b21df27d15cdd521950d5a8b42fa1fbd46457cfd6a8"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.2/delphi-v0.6.2-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "c0ff3ccfcd9aef97da9c570bb9153b75e32c5e582fe64be37b59fecb7b177afc"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.2/delphi-v0.6.2-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "ece25ad7b0358c76e398eb379139578acbc2a8e46d5e597dcfab9f8336ea1c1f"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
