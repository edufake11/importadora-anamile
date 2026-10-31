/* Páginas de producto: leen los datos actuales de productos.js (precios, colores, fotos). */
(function(){
const $=i=>document.getElementById(i),K='anamile_sel';
const D=JSON.parse($('pdata').textContent);
let G=null;try{const id0=D.V[0].id;G=PRODUCTS.find(x=>x.variants.some(v=>v.id===id0))}catch(e){}
const MM=(typeof MIN_MAYOR!=='undefined'&&MIN_MAYOR)||2;
const money=n=>'S/ '+Number(n).toFixed(2);
const esc=t=>String(t).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const swBg=h=>h==='#dff0f7'?'linear-gradient(135deg,#fff 45%,#cfe8f3 55%)':h;
const rd=()=>{try{return JSON.parse(localStorage.getItem(K))||{}}catch(e){return {}}},wr=o=>{try{localStorage.setItem(K,JSON.stringify(o))}catch(e){}};
function rows(g,v){const m=g.variants.length>1;const r=[['Código',v.id],['Categoría',g.cat]];if(m&&v.label)r.push([g.vtype||'Modelo',v.label]);if(v.dim)r.push(['Medidas',v.dim]);if(v.mat)r.push(['Material',v.mat]);return r.map(([k,x])=>'<dt>'+esc(k)+'</dt><dd>'+esc(x)+'</dd>').join('')}
function price(v){if(v.price&&v.wholesale)return '<div class="big">'+money(v.price)+'<small>  precio por menor (1 unidad)</small></div><div class="mayor">Por mayor: '+money(v.wholesale)+' c/u desde '+MM+' unidades en total</div><p>Consulta disponibilidad por WhatsApp.</p>';
if(v.price)return '<div class="big">'+money(v.price)+'<small>  precio por unidad</small></div><p>Consulta disponibilidad por WhatsApp.</p>';
if(v.wholesale)return '<div class="big">'+money(v.wholesale)+'<small>  precio por mayor</small></div><p>Precio por mayor directo. Consulta disponibilidad por WhatsApp.</p>';
return '<div class="ask">Consulta el precio por WhatsApp</div><p>Venta por mayor y por menor.</p>'}
const ttl=(g,v)=>g.name+(g.variants.length>1&&v.label?' · '+v.label:'');
const wa=(g,v)=>'https://wa.me/51906990985?text='+encodeURIComponent('Hola, Importadora Anamile 🎀\nMe interesa este producto: '+ttl(g,v)+' ('+v.id+').\n¿Me confirman precio por mayor y menor y disponibilidad?');
let V;if(G){V=G.variants.map(v=>({img:'../'+v.img,id:v.id,label:v.label,swatch:v.swatch,title:ttl(G,v),rows:rows(G,v),price:price(v),wa:wa(G,v),alt:(G.name+' '+(v.label||'')+' código '+v.id).replace('  ',' ')}))}else V=D.V;
if(G){const vs=G.variants,n=vs.length,kind={Color:'colores',Modelo:'modelos','Tamaño':'tamaños'}[G.vtype]||'opciones';let t=G.name+' '+G.id+(n>1?' disponible en '+n+' '+kind+': '+vs.map(v=>v.label).join(', ')+'.':'.');
const u=a=>[...new Set(a.filter(Boolean))],dims=u(vs.map(v=>v.dim)),mats=u(vs.map(v=>v.mat)),pr=vs.map(v=>v.price).filter(Boolean),wh=vs.map(v=>v.wholesale).filter(Boolean);
if(dims.length)t+=' Medidas: '+(dims.length===1?dims[0]:dims.slice(0,3).join(' · '))+'.';if(mats.length)t+=' Material: '+mats.slice(0,2).join(', ').toLowerCase()+'.';
if(pr.length)t+=' Precio por menor desde '+money(Math.min(...pr))+(wh.length?' y por mayor desde '+money(Math.min(...wh))+' (desde '+MM+' unidades).':'.');else if(wh.length)t+=' Precio por mayor desde '+money(Math.min(...wh))+'.';
const d=document.querySelector('.pp-detail');if(d)d.textContent=t+' Venta por mayor y menor en Galería Plaza Central, Lima.';const dsc=document.querySelector('.pp-info .m-desc');if(dsc&&G.desc)dsc.textContent=G.desc}
let cur=0;
function ui(){const o=rd();$('cnt').textContent='('+Object.values(o).reduce((a,b)=>a+(+b||0),0)+')';const q=o[V[cur].id]||0;$('ppAdd').textContent=q?'✓ En tu selección ('+q+') · Agregar otro':'💗 Agregar a mi selección';const a2=$('ppAdd2');if(a2)a2.textContent=q?'✓ ('+q+') Agregar otro':'💗 Agregar'}
function show(i,animate){cur=i;const v=V[i],im=$('ppImg');document.querySelectorAll('.vbtn').forEach((x,k)=>x.setAttribute('aria-pressed',k===i));
const set=()=>{im.src=v.img;im.alt=v.alt;im.style.opacity=1};if(animate){im.style.opacity=.15;setTimeout(set,160)}else set();
$('ppRows').innerHTML=v.rows;$('ppPrice').innerHTML=v.price;$('ppWa').href=v.wa;const w2=$('ppWa2');if(w2)w2.href=v.wa;$('ppCat').href='../index.html#p='+encodeURIComponent(v.id);
const l=$('vl');if(l)l.lastElementChild.textContent=v.label;ui()}
if(G)document.querySelectorAll('.vbtn').forEach((b,k)=>{const v=G.variants[k];if(!v)return;const im=b.querySelector('img');if(im){im.src='../'+v.img;im.alt=V[k].alt}const sp=b.querySelector('span');if(sp)sp.textContent=v.label;const d=b.querySelector('.dot');if(d&&v.swatch)d.style.background=swBg(v.swatch)});
document.querySelectorAll('.vbtn').forEach(b=>b.onclick=()=>{const i=+b.dataset.i;show(i,true);history.replaceState(null,'','#'+V[i].id)});
const addOne=()=>{const o=rd(),id=V[cur].id;o[id]=(o[id]||0)+1;wr(o);ui()};
$('ppAdd').onclick=addOne;if($('ppAdd2'))$('ppAdd2').onclick=addOne;
const h=decodeURIComponent(location.hash.slice(1)),k0=V.findIndex(v=>v.id===h);show(k0>0?k0:0,false);
(function(){const im=$('ppImg'),m=im.parentNode,f=()=>m.style.setProperty('--bg','url("'+im.currentSrc+'")');im.addEventListener('load',f);if(im.complete)f()})();
const bar=$('ppBar'),acts=document.querySelector('.pp-info .m-actions');if(bar&&acts&&'IntersectionObserver' in window){new IntersectionObserver(es=>{bar.classList.toggle("show",!es[0].isIntersecting)}).observe(acts)}
})();
