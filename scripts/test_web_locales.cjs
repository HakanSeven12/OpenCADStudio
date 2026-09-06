// Run with node scripts/test_web_locales.cjs.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const page = fs.readFileSync(path.join(root, 'web-app.html'), 'utf8');
const script = [...page.matchAll(/<script>([\s\S]*?)<\/script>/g)].map(match => match[1]).join('\n');
const labels = JSON.parse(fs.readFileSync(path.join(root, 'web/locale-labels.json'), 'utf8'));

async function visit(languages, saved, blocked = false) {
  const caption = { textContent: '' };
  const splash = { querySelector: () => caption, classList: { add() {} }, remove() {} };
  const document = { documentElement: {}, getElementById: () => splash, querySelector: () => ({}) };
  vm.runInNewContext(script, {
    document, navigator: { languages, language: languages[0] }, Intl,
    localStorage: { getItem(key) {
      assert.equal(key, 'opencadstudio.settings');
      if (blocked) throw new Error('Storage disabled');
      return saved;
    } },
    fetch: async url => { assert.equal(url, 'locale-labels.json'); return { json: async () => labels }; },
    setTimeout() {},
  });
  await new Promise(setImmediate);
  return { document, caption };
}

(async () => {
  const cases = Object.keys(labels).map(locale => [[locale], undefined, locale]);
  cases.push(
    [['en-US'], JSON.stringify({ settings: { language: 'tr-TR' } }), 'tr-TR'],
    [['fr-CA'], JSON.stringify({ settings: { language: 'system' } }), 'fr-FR'],
    [['zh-HK'], undefined, 'zh-TW'], [['zh-Hans-SG'], undefined, 'zh-CN'],
    [['sv-SE', 'de-AT'], '{invalid', 'de-DE'], [['sv-SE'], undefined, 'en-US'],
  );
  for (const [languages, saved, expected] of cases) {
    const { document, caption } = await visit(languages, saved);
    assert.equal(document.documentElement.lang, expected);
    assert.equal(document.documentElement.dir, expected === 'ar-SA' ? 'rtl' : 'ltr');
    assert.equal(document.title, labels[expected].title);
    assert.equal(caption.textContent, labels[expected].loading);
  }
  const blocked = await visit(['ja-JP'], undefined, true);
  assert.equal(blocked.document.documentElement.lang, 'ja-JP');
  console.log('Web loading labels, saved language, regional fallbacks and blocked storage passed');
})().catch(error => { console.error(error); process.exitCode = 1; });
