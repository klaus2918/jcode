#!/usr/bin/env bash
# Update the Homebrew tap and AUR package for a new release.
#
# Single source of truth shared by the release pipeline and local maintainers:
#   - CI: .github/workflows/release.yml (release job) calls this script with
#     --assets-dir artifacts (already-downloaded build artifacts) and injects
#     the HOMEBREW_DEPLOY_KEY / AUR_SSH_KEY secrets via env.
#   - Local: scripts/update_packages.sh --version v0.1.3 (downloads the release
#     tarballs itself and uses the caller's default SSH credentials).
#
# Usage:
#   scripts/update_packages.sh --version <version-tag> \
#       [--repo owner/repo] [--assets-dir <dir>] \
#       [--homebrew] [--aur] \
#       [--homebrew-repo <owner/repo>] [--aur-package <pkg>]
#
#   Legacy positional form: scripts/update_packages.sh v0.1.3
#
# Flags:
#   --version       Release tag, e.g. v0.5.5 (required)
#   --repo          Upstream repo, defaults to 1jehuang/jcode
#   --assets-dir    Directory with per-platform artifact subdirs (CI layout:
#                   jcode-linux-x86_64/jcode-linux-x86_64.tar.gz, ...). When
#                   omitted the tarballs are downloaded from the release.
#   --homebrew      Only update the Homebrew tap
#   --aur           Only update the AUR package
#   --homebrew-repo Homebrew tap repo, defaults to 1jehuang/homebrew-jcode
#   --aur-package   AUR package name, defaults to jcode-bin
set -euo pipefail

VERSION=""
REPO="1jehuang/jcode"
ASSETS_DIR=""
DO_HOMEBREW=0
DO_AUR=0
HOMEBREW_REPO="1jehuang/homebrew-jcode"
AUR_PACKAGE="jcode-bin"

usage() {
  echo "Usage: $0 --version <version-tag> [--repo owner/repo] [--assets-dir <dir>] [--homebrew|--aur] [--homebrew-repo <repo>] [--aur-package <pkg>]" >&2
  exit 1
}

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --version) VERSION="${2:-}"; shift 2 ;;
    --repo) REPO="${2:-}"; shift 2 ;;
    --assets-dir) ASSETS_DIR="${2:-}"; shift 2 ;;
    --homebrew) DO_HOMEBREW=1; shift ;;
    --aur) DO_AUR=1; shift ;;
    --homebrew-repo) HOMEBREW_REPO="${2:-}"; shift 2 ;;
    --aur-package) AUR_PACKAGE="${2:-}"; shift 2 ;;
    --help|-h) usage ;;
    -*) usage ;;
    *) VERSION="${VERSION:-$1}"; shift ;;
  esac
done

[[ -n "$VERSION" ]] || usage
if [[ "$DO_HOMEBREW" -eq 0 && "$DO_AUR" -eq 0 ]]; then
  DO_HOMEBREW=1
  DO_AUR=1
fi

VERSION_NUM="${VERSION#v}"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}"

retry() {
  local attempts="$1"
  local delay="$2"
  shift 2
  local try=1

  until "$@"; do
    local exit_code=$?
    if [[ "$try" -ge "$attempts" ]]; then
      return "$exit_code"
    fi
    echo "Attempt ${try}/${attempts} failed; retrying in ${delay}s..." >&2
    sleep "$delay"
    try=$((try + 1))
  done
}

echo "Updating packages for $VERSION (repo: $REPO)..."

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

# --- Checksums ---------------------------------------------------------------
# CI hands over the already-built artifacts; local runs download the tarballs.
# Either way the SHA-256 sums must describe exactly the files published under
# the given release tag.
if [[ -n "$ASSETS_DIR" ]]; then
  LINUX_TARBALL="$ASSETS_DIR/jcode-linux-x86_64/jcode-linux-x86_64.tar.gz"
  LINUX_ARM_TARBALL="$ASSETS_DIR/jcode-linux-aarch64/jcode-linux-aarch64.tar.gz"
  MACOS_ARM_TARBALL="$ASSETS_DIR/jcode-macos-aarch64/jcode-macos-aarch64.tar.gz"
  MACOS_INTEL_TARBALL="$ASSETS_DIR/jcode-macos-x86_64/jcode-macos-x86_64.tar.gz"
else
  LINUX_TARBALL="$tmpdir/linux.tar.gz"
  LINUX_ARM_TARBALL="$tmpdir/linux-arm.tar.gz"
  MACOS_ARM_TARBALL="$tmpdir/macos-arm.tar.gz"
  MACOS_INTEL_TARBALL="$tmpdir/macos-intel.tar.gz"
  echo "Downloading release assets for checksums..."
  curl -fsSL "$DOWNLOAD_URL/jcode-linux-x86_64.tar.gz" -o "$LINUX_TARBALL"
  curl -fsSL "$DOWNLOAD_URL/jcode-linux-aarch64.tar.gz" -o "$LINUX_ARM_TARBALL"
  curl -fsSL "$DOWNLOAD_URL/jcode-macos-aarch64.tar.gz" -o "$MACOS_ARM_TARBALL"
  curl -fsSL "$DOWNLOAD_URL/jcode-macos-x86_64.tar.gz" -o "$MACOS_INTEL_TARBALL"
fi

LINUX_SHA=$(sha256sum "$LINUX_TARBALL" | cut -d' ' -f1)
LINUX_ARM_SHA=$(sha256sum "$LINUX_ARM_TARBALL" | cut -d' ' -f1)
MACOS_ARM_SHA=$(sha256sum "$MACOS_ARM_TARBALL" | cut -d' ' -f1)
MACOS_INTEL_SHA=$(sha256sum "$MACOS_INTEL_TARBALL" | cut -d' ' -f1)

echo "  Linux x86_64 SHA256: $LINUX_SHA"
echo "  Linux aarch64 SHA256: $LINUX_ARM_SHA"
echo "  macOS aarch64 SHA256: $MACOS_ARM_SHA"
echo "  macOS x86_64 SHA256: $MACOS_INTEL_SHA"

# --- Homebrew tap ------------------------------------------------------------
if [[ "$DO_HOMEBREW" -eq 1 ]]; then
  echo ""
  echo "Updating Homebrew tap $HOMEBREW_REPO..."
  if [[ -n "${HOMEBREW_DEPLOY_KEY:-}" ]]; then
    mkdir -p ~/.ssh
    printf '%s\n' "$HOMEBREW_DEPLOY_KEY" > ~/.ssh/deploy_key
    chmod 600 ~/.ssh/deploy_key
    export GIT_SSH_COMMAND="ssh -i ~/.ssh/deploy_key -o StrictHostKeyChecking=no"
  fi

  BREW_DIR="$tmpdir/homebrew-jcode"
  retry 3 5 git clone "git@github.com:${HOMEBREW_REPO}.git" "$BREW_DIR"

  cat > "$BREW_DIR/Formula/jcode.rb" <<EOF
class Jcode < Formula
  desc "AI coding agent powered by Claude and ChatGPT"
  homepage "https://github.com/${REPO}"
  version "${VERSION_NUM}"
  license "MIT"

  on_macos do
    on_arm do
      url "${DOWNLOAD_URL}/jcode-macos-aarch64.tar.gz"
      sha256 "${MACOS_ARM_SHA}"

      def install
        bin.install "jcode-macos-aarch64" => "jcode"
      end
    end

    on_intel do
      url "${DOWNLOAD_URL}/jcode-macos-x86_64.tar.gz"
      sha256 "${MACOS_INTEL_SHA}"

      def install
        bin.install "jcode-macos-x86_64" => "jcode"
      end
    end
  end

  on_linux do
    on_intel do
      url "${DOWNLOAD_URL}/jcode-linux-x86_64.tar.gz"
      sha256 "${LINUX_SHA}"

      def install
        libexec.install "jcode-linux-x86_64", "jcode-linux-x86_64.bin"
        libexec.install Dir["libssl.so*"], Dir["libcrypto.so*"] unless Dir["libssl.so*", "libcrypto.so*"].empty?
        (bin/"jcode").write <<~SH
          #!/bin/sh
          exec "#{libexec}/jcode-linux-x86_64" "\$@"
        SH
      end
    end

    on_arm do
      url "${DOWNLOAD_URL}/jcode-linux-aarch64.tar.gz"
      sha256 "${LINUX_ARM_SHA}"

      def install
        bin.install "jcode-linux-aarch64" => "jcode"
      end
    end
  end

  test do
    assert_match "jcode", shell_output("#{bin}/jcode --version")
  end
end
EOF

  (cd "$BREW_DIR" \
    && git config user.name "jcode-release-bot" \
    && git config user.email "release@jcode.dev" \
    && git add Formula/jcode.rb \
    && git commit -m "Update to ${VERSION}" || echo "No changes") \
    && (cd "$BREW_DIR" && git push)
  echo "  Homebrew tap updated"
fi

# --- AUR ---------------------------------------------------------------------
if [[ "$DO_AUR" -eq 1 ]]; then
  echo ""
  echo "Updating AUR package $AUR_PACKAGE..."
  if [[ -n "${AUR_SSH_KEY:-}" ]]; then
    mkdir -p ~/.ssh
    chmod 700 ~/.ssh
    printf '%s\n' "$AUR_SSH_KEY" > ~/.ssh/aur_key
    chmod 600 ~/.ssh/aur_key
    touch ~/.ssh/known_hosts
    chmod 644 ~/.ssh/known_hosts
    retry 3 5 bash -lc 'ssh-keyscan -H aur.archlinux.org >> ~/.ssh/known_hosts'
    export GIT_SSH_COMMAND="ssh -i ~/.ssh/aur_key -o BatchMode=yes -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=$HOME/.ssh/known_hosts -o ConnectTimeout=10 -o ConnectionAttempts=3"
  fi

  AUR_DIR="$tmpdir/jcode-bin-aur"
  retry 3 5 bash -lc "rm -rf '$AUR_DIR' && git clone --depth 1 'ssh://aur@aur.archlinux.org/${AUR_PACKAGE}.git' '$AUR_DIR'"

  LINUX_URL="$DOWNLOAD_URL/jcode-linux-x86_64.tar.gz"

  cat > "$AUR_DIR/PKGBUILD" <<'PKGBUILD_END'
# Maintainer: Jeremy Huang <jeremyhuang55555@gmail.com>
pkgname=jcode-bin
pkgver=VERSION_PLACEHOLDER
pkgrel=1
pkgdesc="AI coding agent powered by Claude and ChatGPT"
arch=('x86_64')
url="https://github.com/REPO_PLACEHOLDER"
license=('MIT')
provides=('jcode')
conflicts=('jcode')
source=("URL_PLACEHOLDER")
sha256sums=('SHA_PLACEHOLDER')

package() {
    install -Dm755 "${srcdir}/jcode-linux-x86_64" "${pkgdir}/usr/lib/jcode/jcode-linux-x86_64"
    install -Dm755 "${srcdir}/jcode-linux-x86_64.bin" "${pkgdir}/usr/lib/jcode/jcode-linux-x86_64.bin"
    if compgen -G "${srcdir}/libssl.so*" >/dev/null; then
        install -Dm644 "${srcdir}"/libssl.so* "${pkgdir}/usr/lib/jcode/"
    fi
    if compgen -G "${srcdir}/libcrypto.so*" >/dev/null; then
        install -Dm644 "${srcdir}"/libcrypto.so* "${pkgdir}/usr/lib/jcode/"
    fi
    mkdir -p "${pkgdir}/usr/bin"
    ln -s /usr/lib/jcode/jcode-linux-x86_64 "${pkgdir}/usr/bin/jcode"
}
PKGBUILD_END
  sed -i "s|VERSION_PLACEHOLDER|${VERSION_NUM}|" "$AUR_DIR/PKGBUILD"
  sed -i "s|REPO_PLACEHOLDER|${REPO}|" "$AUR_DIR/PKGBUILD"
  sed -i "s|URL_PLACEHOLDER|${LINUX_URL}|" "$AUR_DIR/PKGBUILD"
  sed -i "s|SHA_PLACEHOLDER|${LINUX_SHA}|" "$AUR_DIR/PKGBUILD"

  # Generate .SRCINFO without makepkg (AUR uses tab indentation)
  printf 'pkgbase = jcode-bin\n' > "$AUR_DIR/.SRCINFO"
  printf '\tpkgdesc = AI coding agent powered by Claude and ChatGPT\n' >> "$AUR_DIR/.SRCINFO"
  printf '\tpkgver = %s\n' "$VERSION_NUM" >> "$AUR_DIR/.SRCINFO"
  printf '\tpkgrel = 1\n' >> "$AUR_DIR/.SRCINFO"
  printf '\turl = https://github.com/%s\n' "$REPO" >> "$AUR_DIR/.SRCINFO"
  printf '\tarch = x86_64\n' >> "$AUR_DIR/.SRCINFO"
  printf '\tlicense = MIT\n' >> "$AUR_DIR/.SRCINFO"
  printf '\tprovides = jcode\n' >> "$AUR_DIR/.SRCINFO"
  printf '\tconflicts = jcode\n' >> "$AUR_DIR/.SRCINFO"
  printf '\tsource = %s\n' "$LINUX_URL" >> "$AUR_DIR/.SRCINFO"
  printf '\tsha256sums = %s\n' "$LINUX_SHA" >> "$AUR_DIR/.SRCINFO"
  printf '\npkgname = jcode-bin\n' >> "$AUR_DIR/.SRCINFO"

  (cd "$AUR_DIR" \
    && git config user.name "Jeremy Huang" \
    && git config user.email "jeremyhuang55555@gmail.com" \
    && git add PKGBUILD .SRCINFO \
    && git commit -m "Update to ${VERSION}" || echo "No changes")
  (cd "$AUR_DIR" && retry 3 5 git push origin master)
  echo "  AUR package updated"
fi

echo ""
echo "Done! Packages updated to $VERSION"
