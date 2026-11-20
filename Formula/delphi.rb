class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.6.6"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.6/delphi-v0.6.6-aarch64-apple-darwin.tar.gz"
      sha256 "db45bbc75d65c289cd61ead909b897a8bce3660ac4f6103f78bdf1ec8b6ab146"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.6/delphi-v0.6.6-x86_64-apple-darwin.tar.gz"
      sha256 "9f83c88942d518d6c24c2369f6ddc22ad735bb7a6bd3bcf987bb788e2939be81"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.6/delphi-v0.6.6-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "919d379ffdaf84ee851169e8543ede125ab5b571f29abb6f16ce61e773d5da4a"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.6/delphi-v0.6.6-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "f15b624d8a74b17abf86761e7c3824d903d8a40fc8637cc648614cdb1567a939"
    end
  end

  def install
    bin.install "delphi", "delphi-ui"
    # The desktop app (specs/122), on macOS: kept in the prefix; the caveats say how to link it.
    prefix.install "Delphi.app" if OS.mac? && File.directory?("Delphi.app")
  end

  def caveats
    s = <<~EOS
      The Delphi plugin for Claude Code and Copilot CLI runs ~/.local/bin/delphi.
      Link it once (`delphi connect claude-code` offers to do this for you):
        mkdir -p ~/.local/bin && ln -s #{HOMEBREW_PREFIX}/bin/delphi ~/.local/bin/delphi
    EOS
    if OS.mac?
      s += <<~EOS
        The desktop app is in #{opt_prefix}/Delphi.app. Link it where `delphi app` looks:
          mkdir -p ~/Applications && ln -sfn #{opt_prefix}/Delphi.app ~/Applications/Delphi.app
      EOS
    end
    s
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/delphi --version")
  end
end
