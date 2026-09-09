const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

(async () => {
    const source = fs.readFileSync(path.join(__dirname, '../web/clipboard.js'), 'utf8');
    const { copyHistory } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
    const text = 'LINE\n0,0\n10,10\n<unchanged & literal>\n';
    let copied;
    Object.defineProperty(globalThis, 'navigator', { configurable: true, value: {
        clipboard: { writeText: async value => { copied = value; } },
    }});
    assert.equal(await copyHistory(text, 'Copy manually', 'Close'), true);
    assert.equal(copied, text);
    const dialogs = [];
    globalThis.document = {
        body: { append: dialog => dialogs.push(dialog) },
        createElement: tag => ({
            tag, style: {}, events: {},
            setAttribute(key, value) { this[key] = value; },
            append(...children) { this.children = children; },
            addEventListener(event, handler) { this.events[event] = handler; },
            showModal() { this.open = true; },
            focus() { this.focused = true; },
            select() { this.selected = true; },
            close() { this.events.close(); },
            remove() { this.removed = true; },
        }),
    };
    for (const clipboard of [{ writeText: async () => { throw Error('Denied'); } }, undefined]) {
        navigator.clipboard = clipboard;
        assert.equal(await copyHistory(text, 'Copy manually', 'Close'), false);
        const dialog = dialogs.at(-1);
        const [label, field, close] = dialog.children;
        assert.equal(label.textContent, 'Copy manually');
        assert.equal(field.value, text);
        assert.ok(field.readOnly && field.focused && field.selected && dialog.open);
        close.onclick();
        assert.ok(dialog.removed);
    }
    console.log('Browser clipboard: success, denied and unavailable paths passed.');
})().catch(error => { console.error(error); process.exitCode = 1; });
