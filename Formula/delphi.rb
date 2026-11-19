class Delphi < Formula
  desc "Terminal-first multi-agent working environment"
  homepage "https://github.com/jessevondoom/delphi"
  version "0.6.5"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.5/delphi-v0.6.5-aarch64-apple-darwin.tar.gz"
      sha256 "5c47619b4e4143efb8e31f7ec4e5919643711343817ee120c1ca090994a7a019"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.5/delphi-v0.6.5-x86_64-apple-darwin.tar.gz"
      sha256 "bbb0bd7440da454a19e7d0a1f4c892d38c8ed3c115593adef671216d58ba2d8a"
    end
  end
  on_linux do
    on_arm do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.5/delphi-v0.6.5-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "d8e52f4aa4303835419eb353acd7614b494d74a93be47dc2f92b8118c26aa64d"
    end
    on_intel do
      url "https://github.com/jessevondoom/delphi/releases/download/v0.6.5/delphi-v0.6.5-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "680ef3050a42c3017c52600d12e967614a1350b917e4062410698745938da6e3"
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
