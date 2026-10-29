export const targets = {
  "linux-x64": {
    os: "linux",
    cpu: "x64",
    libc: ["glibc"],
    triple: "x86_64-unknown-linux-gnu",
  },
  "darwin-arm64": {
    os: "darwin",
    cpu: "arm64",
    triple: "aarch64-apple-darwin",
  },
  "darwin-x64": { os: "darwin", cpu: "x64", triple: "x86_64-apple-darwin" },
};

export function platformPackage(platform, arch, glibc) {
  const key = `${platform}-${arch}`;
  if (!targets[key])
    throw new Error(
      `Unsupported platform: ${key}. Supported: ${Object.keys(targets).join(", ")}.`,
    );
  if (platform === "linux") {
    if (!glibc)
      throw new Error(
        "Linux musl is not supported by this binary; use a glibc system or build errspec from source.",
      );
    const [major, minor] = glibc.split(".").map(Number);
    if (!(major > 2 || (major === 2 && minor >= 35)))
      throw new Error("This Linux binary requires glibc 2.35 or later.");
  }
  return `@errspec/cli-${key}`;
}
