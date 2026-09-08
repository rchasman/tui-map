// Native HTML keeps source links keyboard-accessible and independent of map input.
export function createInspector(root,onClose) {
  const find=id=>root.querySelector(`#inspector-${id}`);
  find('close').addEventListener('click',onClose);
  let previous;
  return details=>{
    const signature=JSON.stringify(details??null);
    if(signature===previous)return;
    previous=signature;
    root.hidden=!details;
    if(!details)return;
    find('title').textContent=details.label;
    find('source').textContent=`${details.source} · ${details.state}`;
    find('detail').textContent=details.detail;
    find('coordinates').textContent=`${details.lat.toFixed(3)}, ${details.lon.toFixed(3)}`;
    const date=new Date(details.observed*1000);
    find('time').textContent=Number.isFinite(date.valueOf())?date.toISOString().replace('T',' ').replace(/\.\d{3}Z$/,' UTC'):'Unknown';
    const link=find('link');
    let url;
    try {const parsed=new URL(details.url);if(parsed.protocol==='https:'||parsed.protocol==='http:')url=parsed.href;}catch{}
    link.hidden=!url;
    if(url){link.href=url;link.textContent=url;link.title=`${url} (opens in a new tab)`;}
    else {link.removeAttribute('href');link.textContent='';}
  };
}
