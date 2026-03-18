const si = require('systeminformation');

async function check() {
  try {
    const gpu = await si.graphics();
    console.log(JSON.stringify(gpu, null, 2));
  } catch (e) {
    console.error(e);
  }
}

check();
