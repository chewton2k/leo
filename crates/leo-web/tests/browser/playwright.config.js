const { defineConfig, devices } = require('@playwright/test');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const home = process.env.LEO_BROWSER_HOME || fs.mkdtempSync(path.join(os.tmpdir(), 'leo-browser-'));
process.env.LEO_BROWSER_HOME = home;
module.exports = defineConfig({
  testMatch: ['flows.spec.js', 'source-layout.spec.js', 'editor-layout.spec.js', 'sidebar.spec.js', 'explorer-workspace.spec.js', 'margin-ui.spec.js', 'explorer-review.spec.js', 'margin-motion-review.spec.js', 'review-evidence.spec.js', 'routing-chat-review.spec.js', 'tasks-layout.spec.js', 'speech-models.spec.js', 'dictation.spec.js', 'settings.spec.js', 'boards.spec.js', 'calendar.spec.js', 'title-keys.spec.js', 'prefs.spec.js'],
  globalTeardown: './teardown.js',
  fullyParallel: true,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['list'], ['html',{open:'never'}]] : 'list',
  timeout: 30000,
  use: {
    baseURL: 'http://127.0.0.1:31831',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    launchOptions: { args: ['--host-resolver-rules=MAP leo-http.test 127.0.0.1', '--no-proxy-server'] },
  },
  projects: [
    { name: 'phone', use: { ...devices['Pixel 7'] }, testMatch: ['flows.spec.js'], grep: /on a phone/ },
    { name: 'desktop', use: { browserName: 'chromium' } },
    { name: 'webkit', use: { browserName: 'webkit' }, testMatch: ['settings.spec.js', 'speech-models.spec.js', 'dictation.spec.js', 'editor-layout.spec.js', 'sidebar.spec.js', 'explorer-workspace.spec.js', 'margin-ui.spec.js', 'explorer-review.spec.js', 'margin-motion-review.spec.js', 'review-evidence.spec.js', 'routing-chat-review.spec.js', 'boards.spec.js', 'calendar.spec.js', 'title-keys.spec.js'] },
  ],
  webServer: {
    command: 'node server.js',
    url: 'http://127.0.0.1:31831',
    reuseExistingServer: false,
    env: { LEO_HOME: home, LEO_INSTALL_NO_MODEL: '1', LEO_NO_UPDATE_CHECK: '1', LEO_NO_UI_PREFS: '1', LEO_NO_MICROPHONE: '1', LEO_NO_OPEN: '1', LEO_TEST_FAKE_AI: JSON.stringify({ quote: 'just checks if it matches', text: 'In plain text? If this table leaks, every account goes.' }) },
  },
});
