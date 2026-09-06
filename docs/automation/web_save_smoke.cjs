// Serve a Trunk build, then run: node docs/automation/web_save_smoke.cjs URL [chromium|firefox]
// Requires Playwright and its browser binaries. Uses a fresh, disposable browser profile.
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const playwright = require('playwright');

(async () => {
    const [url = 'http://127.0.0.1:8080', engine = 'chromium'] = process.argv.slice(2);
    assert(['chromium', 'firefox'].includes(engine));
    const output = await fs.mkdtemp(path.join(os.tmpdir(), 'ocs-web-save-'));
    const browser = await playwright[engine].launch({
        headless: true,
        ...(engine === 'chromium'
            ? {args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader']}
            : {firefoxUserPrefs: {'webgl.force-enabled': true}}),
    });
    try {
        for (const format of ['DWG 2018', 'DXF 2018']) {
            const context = await browser.newContext({
                viewport: {width: 1280, height: 900}, deviceScaleFactor: 1,
            });
            const page = await context.newPage();
            page.setDefaultTimeout(30000);
            const errors = [];
            page.on('pageerror', error => errors.push(String(error)));
            page.on('console', message => {
                if (/panicked|Buffer is not mapped/.test(message.text())) errors.push(message.text());
            });
            await page.addInitScript(format => localStorage.setItem('opencadstudio.settings',
                JSON.stringify({settings: {language: 'en-US', default_save_format: format, savetime_min: 0}})), format);
            await page.goto(url);
            await page.waitForFunction(() => window.wasmBindings?.ocs_control_submit, undefined, {timeout: 120000});
            const raw = request => page.evaluate(async request => {
                const api = window.wasmBindings;
                const ticket = api.ocs_control_submit(JSON.stringify(request));
                for (let i = 0; i < 600; i++) {
                    const reply = api.ocs_control_take(ticket);
                    if (reply) return JSON.parse(reply);
                    await new Promise(resolve => setTimeout(resolve, 50));
                }
                throw new Error('Control timed out: ' + JSON.stringify(request));
            }, request);
            let serial = 0;
            async function act(request) {
                const state = await raw({op: 'state'});
                const id = 'save-smoke-' + ++serial;
                let reply = await raw({...request, request_id: id,
                    document_id: state.document_id, revision: state.revision});
                for (let i = 0; ['accepted', 'running'].includes(reply.status) && i < 600; i++) {
                    await page.waitForTimeout(50);
                    reply = await raw({op: 'operation', request_id: id});
                }
                assert(reply.ok, JSON.stringify(reply));
                return reply;
            }
            async function save(command, label) {
                const downloaded = page.waitForEvent('download', {timeout: 30000});
                await act({op: 'run', cmd: command});
                if (command === 'SAVEAS') {
                    assert.equal((await raw({op: 'state'})).modal, 'SaveDialog');
                    await page.screenshot({path: path.join(output, `${format}-dialog.png`)});
                    // Save As button in the fixed 1280x900, English canvas layout.
                    await page.mouse.click(585, 521);
                }
                const download = await downloaded;
                const destination = path.join(output, `${label}-${download.suggestedFilename()}`);
                await download.saveAs(destination);
                const bytes = await fs.readFile(destination);
                assert(bytes.length > 1000, 'Drawing download is empty');
                assert.deepEqual(errors, []);
                return {name: download.suggestedFilename(), bytes};
            }
            let state = await raw({op: 'state'});
            if (state.modal) await act({op: 'action', name: 'close_modal'});
            await act({op: 'new'});
            await act({op: 'run', cmd: 'LINE 0,0 10,0'});
            await act({op: 'action', name: 'zoom_extents'});
            await page.screenshot({path: path.join(output, `${format}-drawing.png`)});
            const first = await save('QSAVE', 'quick');
            if (format.startsWith('DWG')) {
                const offset = first.bytes.indexOf(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
                assert(offset >= 0, 'DWG preview PNG is missing');
                const preview = await page.evaluate(async bytes => {
                    const image = await createImageBitmap(new Blob([new Uint8Array(bytes)], {type: 'image/png'}));
                    const canvas = document.createElement('canvas');
                    canvas.width = image.width; canvas.height = image.height;
                    const ctx = canvas.getContext('2d');
                    ctx.drawImage(image, 0, 0);
                    const rgba = ctx.getImageData(0, 0, image.width, image.height).data;
                    const colors = new Set();
                    for (let i = 0; i < rgba.length; i += 4) colors.add(rgba.slice(i, i + 4).join(','));
                    image.close();
                    return {width: canvas.width, height: canvas.height, colors: colors.size};
                }, [...first.bytes.subarray(offset)]);
                assert.equal(preview.width, 256);
                const [width, height] = (await raw({op: 'state'})).viewport_size;
                assert(Math.abs(preview.height - Math.round(256 * height / width)) <= 1);
                assert(preview.colors > 1, 'Preview lost the drawing');
            }
            await save('SAVE', 'save');
            const savedAs = await save('SAVEAS', 'save-as');
            // A denied canvas read must still allow the drawing to be saved.
            const canvasRead = await page.evaluateHandle(() => CanvasRenderingContext2D.prototype.getImageData);
            await page.evaluate(() => {
                CanvasRenderingContext2D.prototype.getImageData = () => {
                    throw new DOMException('Read denied for test', 'SecurityError');
                };
            });
            await save('QSAVE', 'capture-denied');
            await canvasRead.evaluate(original => CanvasRenderingContext2D.prototype.getImageData = original);
            await canvasRead.dispose();
            const oldId = (await raw({op: 'state'})).document_id;
            await act({op: 'action', name: 'close_document'});
            // Reparse the saved browser-private copy, as a Recent Documents entry does.
            await act({op: 'open', path: savedAs.name});
            state = await raw({op: 'state'});
            const entities = await raw({op: 'entities'});
            assert.notEqual(state.document_id, oldId, 'File did not reopen');
            assert.equal(entities.total, 1, JSON.stringify(entities));
            assert.deepEqual(errors, []);
            await page.screenshot({path: path.join(output, `${format}-reopened.png`)});
            console.log(`${engine}: ${format} QSAVE, SAVE, SAVEAS, denied capture, reopen passed`);
            await context.close();
        }
        console.log('Artifacts:', output);
    } finally {
        await browser.close();
    }
})().catch(error => { console.error(error); process.exitCode = 1; });
