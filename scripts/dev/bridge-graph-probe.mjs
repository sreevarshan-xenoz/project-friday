// Dev probe: watch InvestigationGraph goal changes on the live bridge (node 22+).
// Track InvestigationGraph goal changes on the live bridge (~40s ≈ 3 cycles).
const ws = new WebSocket('ws://127.0.0.1:18433');
let lastGoal = null;
let frames = 0;

ws.onmessage = (msg) => {
  const f = JSON.parse(String(msg.data));
  if (f.kind === 'hello') {
    ws.send(JSON.stringify({ type: 'hello_response', capabilities: { metrics: true } }));
    return;
  }
  if (f.kind === 'snapshot' || (f.kind === 'delta' && f.dashboard?.graph)) {
    const g = f.dashboard.graph;
    if (!g) return;
    frames += 1;
    if (g.goal !== lastGoal) {
      lastGoal = g.goal;
      console.log('GOAL @seq' + f.sequence + ' phase=' + f.dashboard.phase);
      console.log('   ' + g.goal);
      console.log('   channels=' + JSON.stringify(g.requirements.map((r) => r.channel)) +
        ' phases=' + JSON.stringify(g.phases));
    }
  }
};

ws.onerror = () => process.exit(1);
ws.onclose = () => { console.log('CLOSED after ' + frames + ' graph frames'); process.exit(0); };
setTimeout(() => { ws.close(); }, 40000);
