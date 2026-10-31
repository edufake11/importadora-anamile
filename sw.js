/* Anamile: caché para que el sitio abra rápido y funcione con mala señal. */
const V='anamile-v9',IMG='anamile-img-v9',CORE=['./','index.html','estilos.css?v=9','productos.js?v=9','logo-sm.png','logo-anamile.png','favicon.svg'];
self.addEventListener('install',e=>{e.waitUntil(caches.open(V).then(c=>Promise.allSettled(CORE.map(u=>c.add(u)))).then(()=>self.skipWaiting()))});
self.addEventListener('activate',e=>{e.waitUntil(caches.keys().then(ks=>Promise.all(ks.filter(k=>k!==V&&k!==IMG).map(k=>caches.delete(k)))).then(()=>self.clients.claim()))});
self.addEventListener('fetch',e=>{const r=e.request;if(r.method!=='GET')return;const u=new URL(r.url);if(u.origin!==location.origin)return;
if(/\/img\//.test(u.pathname)||/\.(png|jpe?g|webp|svg|ico)$/i.test(u.pathname)){e.respondWith(caches.open(IMG).then(async c=>{const hit=await c.match(r);if(hit)return hit;try{const res=await fetch(r);if(res.ok){c.put(r,res.clone());c.keys().then(k=>{if(k.length>500)c.delete(k[0])})}return res}catch(err){return hit||Response.error()}}));return}
e.respondWith(fetch(r).then(res=>{if(res.ok){const cp=res.clone();caches.open(V).then(c=>c.put(r,cp))}return res}).catch(()=>caches.match(r).then(h=>h||caches.match('index.html'))))});
