// Run with node scripts/test_site.cjs dist/site.js.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(process.argv[2] || 'dist/site.js', 'utf8');

function visit(language, pathname = '/', saved, blocked = false) {
  let redirected, click;
  const context = {
    navigator: { language },
    location: { pathname, search: '?ref=test', hash: '#formats', replace: url => { redirected = url; } },
    document: { addEventListener: (type, listener) => { click = listener; } },
    localStorage: {
      getItem: () => { if (blocked) throw new Error('Storage disabled'); return saved; },
      setItem: (key, value) => { if (blocked) throw new Error('Storage disabled'); saved = value; },
    },
  };
  vm.runInNewContext(source, context);
  return { context, redirected, choose: tag => click({ target: { closest: () => ({ hreflang: tag }) } }), saved: () => saved };
}

for (const [language, expected] of Object.entries({
  'tr-TR': 'tr-TR', 'tr-CY': 'tr-TR', 'TR': 'tr-TR', 'pt-PT': 'pt-BR',
  'zh-Hant': 'zh-TW', 'zh-Hant-HK': 'zh-TW', 'zh-HK': 'zh-TW', 'zh-MO': 'zh-TW',
  'zh-Hans-SG': 'zh-CN', 'en-GB': 'en-US', 'sv-SE': 'en-US', '': 'en-US',
})) {
  const result = visit(language);
  assert.equal(result.context.preferredLocale(language), expected);
  assert.equal(result.redirected, expected === 'en-US' ? undefined : `/${expected}/?ref=test#formats`);
}
const context = visit('en-US').context;
for (const locale of vm.runInNewContext('SITE_LOCALES', context)) {
  assert.equal(context.preferredLocale(locale), locale);
}
assert.equal(visit('tr-TR', '/', 'en-US').redirected, undefined);
assert.equal(visit('sv-SE', '/', 'tr-TR').redirected, '/tr-TR/?ref=test#formats');
assert.equal(visit('tr-TR', '/', '../../unsafe').redirected, '/tr-TR/?ref=test#formats');
assert.equal(visit('tr-TR', '/en-US/', undefined, true).redirected, undefined);
assert.equal(visit('en-US', '/tr-TR/').redirected, undefined);
assert.equal(visit('tr-TR', '/', undefined, true).redirected, '/tr-TR/?ref=test#formats');
assert.equal(visit('tr-TR', '/index.html').redirected, '/tr-TR/?ref=test#formats');
const choice = visit('en-US');
choice.choose('tr-TR');
assert.equal(choice.saved(), 'tr-TR');
visit('en-US', '/', undefined, true).choose('en-US');
console.log('Website language matching, redirects, manual choices and blocked storage passed');
