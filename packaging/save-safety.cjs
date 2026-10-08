// Browser regression suite: no generation, real HTTP API, isolated temporary data.
// Usage: node packaging/save-safety.cjs <release web bundle> [browser executable]
// Install Playwright separately, or point PLAYWRIGHT_MODULE at an existing package.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const net = require('node:net');
const {spawn} = require('node:child_process');
const {randomUUID} = require('node:crypto');
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const gate = () => { let resolve; const promise = new Promise(r => { resolve = r; }); return {promise, resolve}; };
const pause = ms => new Promise(r => setTimeout(r, ms));
async function availablePort() {
    const server = net.createServer();
    await new Promise(r => server.listen(0, '127.0.0.1', r));
    const port = server.address().port;
    await new Promise(r => server.close(r));
    return port;
}
async function until(check, label) {
    for (let i=0; i<100; i++) { if (await check()) return; await pause(50); }
    throw Error(`Timed out: ${label}`);
}
(async () => {
    const bundle = path.resolve(process.argv[2] || 'target/dx/vmq_mvp/release/web');
    const base = fs.mkdtempSync(path.join(os.tmpdir(), 'exam-save-safety-'));
    const port = await availablePort();
    const url = `http://127.0.0.1:${port}`;
    const env = {...process.env, IP:'127.0.0.1', PORT:String(port), PUBLIC_PORT:'', PUBLIC_HOST:'',
        DATA_DIR:path.join(base,'data'), GEMINI_API_KEY:'offline-test-placeholder',
        VOICES_PATH:path.join(base,'voices.json'), DIOXUS_PUBLIC_PATH:path.join(bundle,'public'), AUDIO_RETENTION_HOURS:'0'};
    const server = spawn(path.join(bundle, process.platform === 'win32' ? 'server.exe' : 'server'),
        ['--config-dir', base, '--no-open', '--non-interactive'], {cwd:base, env, windowsHide:true, stdio:['ignore','pipe','pipe']});
    const log = fs.createWriteStream(path.join(base,'server.log')); server.stdout.pipe(log); server.stderr.pipe(log);
    let browser;
    const pendingGates = [];
    try {
        await until(async () => { try { return (await fetch(url)).ok; } catch { return false; } }, 'server startup');
        browser = await chromium.launch({headless:true, ...(process.argv[3] ? {executablePath:process.argv[3]} : {})});
        const context = await browser.newContext();
        const errors = [];
        const routes = new Map();
        context.on('page', page => {
            page.on('pageerror', e => errors.push(e.message));
            page.on('request', request => {
                const match = new URL(request.url()).pathname.match(/^\/api\/([a-z_]+)\d+$/);
                if (match) routes.set(match[1], request.url());
            });
        });
        await context.route(/generate_passage|generate_task|suggest_topic|start_.*audio|summarize_topics|design_voice/, route => {
            errors.push('Unexpected generation request'); return route.abort();
        });
        const title = page => page.locator('.exam-setup input.form-input');
        const ready = async page => {
            const response = page.waitForResponse(r => r.url().includes('/api/list_exams'));
            await page.goto(url+'/exam'); await response;
            await title(page).waitFor();
        };
        const waitTitle = (page, text) => until(async()=>await title(page).inputValue() === text, `title ${text}`);
        const saved = page => page.getByText(/^Saved at /).waitFor();
        const row = (page, name) => page.locator('.exam-library tbody tr').filter({hasText:name});
        const open = async (page, name) => { await row(page,name).getByRole('button',{name:'Open',exact:true}).click(); await waitTitle(page,name); };
        const editAndSave = async (page, text) => {
            const result = page.waitForResponse(r=>r.url().includes('/api/save_exam'));
            await title(page).fill(text); await page.getByRole('button',{name:'Save',exact:true}).click();
            const data = await (await result).json(); return data;
        };
        const api = async (name, body) => {
            const endpoint = routes.get(name);
            assert(endpoint, `Observed API route ${name}`);
            const response = await context.request.post(endpoint,{data:body});
            assert(response.ok(), `${name}: ${response.status()}`);
            return response.json();
        };
        const a = await context.newPage(); await ready(a);
        const requestPromise = a.waitForRequest(r=>r.url().includes('/api/save_exam'));
        await editAndSave(a,'Exam A'); await saved(a);
        const template = (await requestPromise).postDataJSON().request;
        const aid = template.saved.exam.id;
        const ids = {};
        for (const name of ['Exam B','Exam C']) {
            const request = structuredClone(template);
            request.saved.exam.id = randomUUID(); request.saved.exam.title = name;
            request.mutation_id = randomUUID(); ids[name] = request.saved.exam.id;
            assert((await api('save_exam',{request})).Saved);
        }
        await ready(a); await open(a,'Exam A');
        const b = await context.newPage(); await ready(b); await open(b,'Exam A');
        await editAndSave(a,'A from first tab'); await saved(a);
        assert((await editAndSave(b,'A from stale tab')).Conflict);
        await b.getByRole('button',{name:'Overwrite server version'}).click();
        await b.getByRole('dialog').waitFor();
        await editAndSave(a,'A changed after confirmation'); await saved(a);
        await b.getByRole('button',{name:'Confirm overwrite'}).click();
        await b.getByRole('button',{name:'Overwrite server version'}).waitFor();
        assert.equal((await api('load_exam',{id:aid})).exam.title,'A changed after confirmation');
        await b.getByRole('button',{name:'Overwrite server version'}).click();
        await b.getByRole('button',{name:'Confirm overwrite'}).click(); await saved(b);
        assert.equal((await api('load_exam',{id:aid})).exam.title,'A from stale tab');
        console.log('PASS: two tabs, explicit overwrite, another edit before confirmation');

        // Delay one save response; keep editing A and open B before it completes.
        const held = gate(), release = gate(); pendingGates.push(release);
        let heldOnce = false;
        await b.route(/\/api\/save_exam/, async route => {
            if (!heldOnce) { heldOnce = true; held.resolve(); await release.promise; }
            await route.continue();
        });
        await title(b).fill('A delayed'); await b.getByRole('button',{name:'Save',exact:true}).click(); await held.promise;
        await title(b).fill('A latest while saving');
        await open(b,'Exam B');
        assert.equal(await title(b).inputValue(),'Exam B');
        release.resolve();
        await until(async()=>(await api('load_exam',{id:aid})).exam.title === 'A latest while saving','background latest snapshot');
        await b.unroute(/\/api\/save_exam/);
        assert.equal(await title(b).inputValue(),'Exam B');
        console.log('PASS: switch immediately, latest old-exam edits save in background');

        // The server commits, but the response is lost; retry must use the original mutation.
        await open(b,'A latest while saving');
        let lostRequest;
        await b.route(/\/api\/save_exam/, async route => {
            lostRequest = route.request().postDataJSON();
            await route.fetch(); await route.abort('internetdisconnected');
        },{times:1});
        await title(b).fill('A lost acknowledgement'); await b.getByRole('button',{name:'Save',exact:true}).click();
        await b.getByRole('button',{name:'Save / Retry'}).waitFor();
        const revisionAfterLost = (await api('load_exam',{id:aid})).revision;
        const retryPromise = b.waitForRequest(r=>r.url().includes('/api/save_exam'));
        await b.getByRole('button',{name:'Save / Retry'}).click(); await saved(b);
        assert.deepEqual((await retryPromise).postDataJSON(),lostRequest);
        assert.equal((await api('load_exam',{id:aid})).revision,revisionAfterLost);
        console.log('PASS: lost acknowledgement retries identical payload without another revision');

        // Offline failure stays visible on the other route, with the correct local draft.
        await context.setOffline(true);
        await title(b).fill('A offline draft'); await b.getByRole('button',{name:'New exam',exact:true}).click();
        await b.getByRole('button',{name:'Save / Retry'}).waitFor();
        await b.getByRole('link',{name:'One part',exact:true}).click();
        await b.locator('.session-draft').filter({hasText:'A offline draft'}).waitFor();
        await context.setOffline(false);
        await b.getByRole('button',{name:'Open draft',exact:true}).click(); await waitTitle(b,'A offline draft');
        await b.getByRole('button',{name:'Save / Retry'}).click(); await saved(b);
        console.log('PASS: offline draft survives switching routes and can be reopened and retried');

        // Control request ordering: C completes first, then a late error for B.
        const opened = gate(), late = gate(), lateHandled = gate(); pendingGates.push(late);
        await b.route(/\/api\/load_exam/, async route => {
            if (route.request().postDataJSON().id === ids['Exam B']) {
                opened.resolve(); await late.promise; await route.fulfill({status:500,body:'late fixture error'}); lateHandled.resolve();
            } else await route.continue();
        });
        await row(b,'Exam B').getByRole('button',{name:'Open',exact:true}).click(); await opened.promise;
        await open(b,'Exam C'); late.resolve(); await lateHandled.promise;
        await b.unroute(/\/api\/load_exam/);
        await b.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
        assert.equal(await title(b).inputValue(),'Exam C');
        assert.equal(await b.getByText(/Could not open the exam/).count(),0);
        console.log('PASS: stale open errors cannot replace the newest open');

        // A pending open is cancelled by editing the current exam or starting a new one.
        for (const action of ['edit','new']) {
            const entered = gate(), continueOpen = gate(); pendingGates.push(continueOpen);
            await b.route(/\/api\/load_exam/, async route => {
                entered.resolve(); await continueOpen.promise; await route.continue();
            },{times:1});
            await row(b,'Exam B').getByRole('button',{name:'Open',exact:true}).click(); await entered.promise;
            if (action === 'new') await b.getByRole('button',{name:'New exam',exact:true}).click();
            await title(b).fill(`Cancelled open by ${action}`);
            const lateResponse = b.waitForResponse(r=>r.url().includes('/api/load_exam'));
            continueOpen.resolve(); await lateResponse;
            await b.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
            assert.equal(await title(b).inputValue(),`Cancelled open by ${action}`);
        }
        console.log('PASS: editing and New exam invalidate pending opens');

        // Observe the delete route through the UI, then delete an open exam from another tab.
        // Save the current unarmed draft so it does not leave unrelated controls in the panel.
        await b.getByRole('button',{name:'Save',exact:true}).click(); await saved(b);
        await row(b,'Exam B').getByRole('button',{name:'Delete',exact:true}).click();
        const deletedResponse = b.waitForResponse(r=>r.url().includes('/api/delete_exam'));
        await b.getByRole('button',{name:'Confirm delete',exact:true}).click(); await deletedResponse;
        const list = await api('list_exams',{});
        const currentA = list.find(e=>e.id===aid);
        await open(b,currentA.title);
        const remoteDelete = await api('delete_exam',{id:aid,expected_revision:currentA.revision});
        assert.equal(remoteDelete,'Deleted');
        assert.equal(await editAndSave(b,'Draft after remote deletion'),'Deleted');
        await title(b).fill('More edits after remote deletion');
        await b.getByRole('button',{name:'Save as new exam'}).click(); await saved(b);
        const afterCopy = await api('list_exams',{});
        assert(!afterCopy.some(e=>e.id===aid));
        assert.equal(await row(b,currentA.title).count(),0,'deleted exam is removed from the visible library');
        assert(afterCopy.some(e=>e.title==='More edits after remote deletion' && e.id!==aid));
        console.log('PASS: deleted exam stays deleted; its local draft can be saved under a new ID');

        // Editing while a confirmed delete is in flight must keep the later edits.
        const deleteEntered = gate(), finishDelete = gate(); pendingGates.push(finishDelete);
        await b.route(/\/api\/delete_exam/, async route => {
            deleteEntered.resolve(); await finishDelete.promise; await route.continue();
        },{times:1});
        await row(b,'More edits after remote deletion').getByRole('button',{name:'Delete',exact:true}).click();
        await b.getByRole('button',{name:'Confirm delete',exact:true}).click(); await deleteEntered.promise;
        await title(b).fill('Edited while deleting');
        const deleteFinished = b.waitForResponse(r=>r.url().includes('/api/delete_exam'));
        finishDelete.resolve(); await deleteFinished;
        await b.getByRole('button',{name:'Save as new exam'}).waitFor();
        await title(b).fill('Edited again after deleting');
        await b.getByRole('button',{name:'New exam',exact:true}).click();
        await b.locator('.session-draft').filter({hasText:'Edited again after deleting'}).getByRole('button',{name:'Open draft'}).click();
        await waitTitle(b,'Edited again after deleting');
        await b.getByRole('button',{name:'Save as new exam'}).click(); await saved(b);
        console.log('PASS: edits during and after a confirmed deletion survive navigation');

        await b.getByRole('button',{name:'New exam',exact:true}).click();
        await title(b).fill('Unarmed local draft'); await b.getByRole('button',{name:'New exam',exact:true}).click();
        await b.locator('.session-draft').filter({hasText:'Unarmed local draft'}).getByRole('button',{name:'Open draft'}).click();
        await waitTitle(b,'Unarmed local draft');
        const warns = await b.evaluate(()=>!window.dispatchEvent(new Event('beforeunload',{cancelable:true})));
        assert(warns,'beforeunload protects unsaved in-session draft');
        await b.screenshot({path:path.resolve('target/save-safety-browser.png'),fullPage:true});
        assert.deepEqual(errors,[]);
        console.log('PASS: unarmed drafts and beforeunload warning; no browser runtime errors');
    } finally {
        for (const item of pendingGates) item.resolve();
        await browser?.close();
        try {
            const info = JSON.parse(fs.readFileSync(path.join(base,'data','instance.json'),'utf8'));
            const response = await fetch(url+'/instance/stop',{method:'POST',headers:{'x-listening-exam-generator-stop':info.stop_token}});
            if (!response.ok) server.kill();
        } catch { server.kill(); }
        const stopped = new Promise(r=>server.once('exit',r));
        if (server.exitCode === null) await Promise.race([stopped,pause(6000).then(()=>server.kill())]);
        console.log(`Test data and log: ${base}`);
    }
})().catch(error=>{console.error(error);process.exitCode=1;});
