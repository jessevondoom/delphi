// The info document `delphi chat` hands the UI, for the fake daemon.
const roles = {
  orchestrator: { name: 'athena', chassis: '#a855f7', face: '•─•' },
  architect: { name: 'daedalus', chassis: '#38bdf8', face: '•_•' },
  builder: { name: 'hephaestus', chassis: '#f97316', face: '•─•' },
  'red-team': { name: 'ares', chassis: '#ef4444', face: '-_-' },
  strategist: { name: 'odysseus', chassis: '#22c55e', face: '•_•' },
  scout: { name: 'hermes', chassis: '#eab308', face: '•••' },
};
console.log(JSON.stringify({ console_socket: process.argv[2], narrator_socket: process.argv[2], workspace: '.', mission: null, version: '0.5', cwd: process.cwd(), roles }));
