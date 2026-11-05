export default {
  testDir: ".",
  testMatch: "*.spec.mjs",
  use: {headless: true},
  projects: [
    {name: "first", use: {browserName: "chromium"}},
    {name: "second", use: {browserName: "chromium"}},
  ],
};
