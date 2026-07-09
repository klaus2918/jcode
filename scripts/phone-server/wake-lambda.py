import boto3, json, urllib.request

INSTANCE_ID = "i-08214cf66cd3f80c7"
TOKEN = "REPLACE_WITH_WAKE_TOKEN"  # generate: openssl rand -hex 16
HOST = "54.196.207.97"
GATEWAY = f"http://{HOST}:7643/health"
PAIR = f"http://{HOST}:7644/pair-code"

def gateway_healthy():
    try:
        with urllib.request.urlopen(GATEWAY, timeout=3) as r:
            return r.status == 200
    except Exception:
        return False

def fetch_pair_code():
    try:
        with urllib.request.urlopen(f"{PAIR}?t={TOKEN}", timeout=35) as r:
            return json.loads(r.read().decode())
    except Exception as e:
        return {"error": str(e)}

def handler(event, context):
    qs = event.get("queryStringParameters") or {}
    if qs.get("t") != TOKEN:
        return {"statusCode": 403, "body": "forbidden"}

    ec2 = boto3.client("ec2", region_name="us-east-1")
    inst = ec2.describe_instances(InstanceIds=[INSTANCE_ID])["Reservations"][0]["Instances"][0]
    state = inst["State"]["Name"]

    if qs.get("check") == "1":
        healthy = state == "running" and gateway_healthy()
        return {"statusCode": 200,
                "headers": {"Content-Type": "application/json", "Cache-Control": "no-store"},
                "body": json.dumps({"state": state, "healthy": healthy})}

    if qs.get("pair") == "1":
        if state != "running":
            return {"statusCode": 200, "headers": {"Content-Type": "application/json", "Cache-Control": "no-store"},
                    "body": json.dumps({"error": f"instance {state}, wake it first"})}
        return {"statusCode": 200, "headers": {"Content-Type": "application/json", "Cache-Control": "no-store"},
                "body": json.dumps(fetch_pair_code())}

    started = False
    if state == "stopped":
        ec2.start_instances(InstanceIds=[INSTANCE_ID])
        started = True

    html = """<!doctype html><html><head>
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>jcode server</title>
<style>
body{font-family:-apple-system,system-ui;background:#101314;color:#eee;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0}
.card{text-align:center;padding:32px;max-width:360px}
h1{color:#4DD9A6;font-size:1.6em;margin-bottom:8px}
#s{font-size:1.05em;line-height:1.5}
.dot{display:inline-block;width:10px;height:10px;border-radius:50%;margin-right:8px;background:#e6b450}
.ok .dot{background:#4DD9A6}.ok #s b{color:#4DD9A6}
.spin{opacity:.75;font-size:.9em;margin-top:14px}
#pairbtn{display:none;margin-top:22px;background:#4DD9A6;color:#0c0f10;border:none;border-radius:12px;padding:14px 22px;font-size:1.05em;font-weight:600}
#pairout{margin-top:16px;font-size:1em;line-height:1.6}
#pairout .code{font-size:1.9em;letter-spacing:.18em;color:#4DD9A6;font-weight:700}
#pairout a{color:#4DD9A6}
</style></head><body><div class="card" id="c">
<h1>jcode server</h1>
<p id="s"><span class="dot"></span>__MSG__</p>
<p class="spin" id="hint">checking every 5s…</p>
<button id="pairbtn" onclick="pair()">Pair this phone</button>
<div id="pairout"></div>
<script>
const t = new URLSearchParams(location.search).get('t');
async function poll(){
  try{
    const r = await fetch(location.pathname + '?t=' + t + '&check=1', {cache:'no-store'});
    const j = await r.json();
    const s = document.getElementById('s'), c = document.getElementById('c');
    if(j.healthy){
      c.classList.add('ok');
      s.innerHTML = '<span class="dot"></span><b>Ready.</b> Open the jcode app now.';
      document.getElementById('hint').textContent = 'server is up';
      document.getElementById('pairbtn').style.display = 'inline-block';
      return;
    } else {
      s.innerHTML = '<span class="dot"></span>Instance: ' + j.state + ' · gateway warming up…';
    }
  }catch(e){}
  setTimeout(poll, 5000);
}
async function pair(){
  const o = document.getElementById('pairout');
  o.textContent = 'generating code…';
  try{
    const r = await fetch(location.pathname + '?t=' + t + '&pair=1', {cache:'no-store'});
    const j = await r.json();
    if(j.code){
      o.innerHTML = '<div class="code">' + j.code.slice(0,3) + ' ' + j.code.slice(3) + '</div>' +
        '<div>host ' + j.host + ':' + j.port + ' · expires in 5 min</div>' +
        '<div style="margin-top:10px"><a href="' + j.uri + '">Open in jcode app</a></div>';
    } else { o.textContent = 'error: ' + (j.error || 'unknown'); }
  }catch(e){ o.textContent = 'error: ' + e; }
}
poll();
</script></div></body></html>"""
    msg = "Starting the server…" if started else ("Instance is " + state + "…")
    html = html.replace("__MSG__", msg)
    return {"statusCode": 200, "headers": {"Content-Type": "text/html", "Cache-Control": "no-store"}, "body": html}
