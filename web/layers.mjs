const groups=[
  ['Map',[['b','Borders'],['s','States'],['y','Counties'],['c','Cities']]],
  ['Live feeds',[['e','Quakes','quakes','USGS'],['d','Hazards','hazards','NASA EONET'],['a','Aircraft','aircraft','adsb.lol'],['t','Satellites','satellites','CelesTrak']]],
  ['Display',[['L','Labels'],['p','Population']]],
];

export function createLayerPicker(root,command){
  const button=root.querySelector('#layers-toggle'),panel=root.querySelector('#layers-panel');
  const rows=[];
  for(const [heading,items] of groups){
    const fieldset=document.createElement('fieldset'),legend=document.createElement('legend');
    legend.textContent=heading;fieldset.append(legend);
    for(const [key,name,kind,source] of items){
      const label=document.createElement('label'),input=document.createElement('input');
      input.type='checkbox';input.dataset.key=key;
      const text=document.createElement('span'),title=document.createElement('span'),status=document.createElement('small'),shortcut=document.createElement('kbd');
      title.textContent=name;shortcut.textContent=key;text.append(title,status);label.append(input,text,shortcut);
      input.addEventListener('change',()=>command(key));
      fieldset.append(label);rows.push({key,name,kind,source,input,status,display:heading==='Display'});
    }
    panel.append(fieldset);
  }
  function open(value){panel.hidden=!value;button.setAttribute('aria-expanded',String(value));}
  button.addEventListener('click',()=>open(panel.hidden));
  document.addEventListener('pointerdown',event=>{if(!root.contains(event.target))open(false);});
  document.addEventListener('keydown',event=>{
    if(event.key==='Escape'&&!panel.hidden){open(false);button.focus({preventScroll:true});}
  });
  let signature='';
  return state=>{
    const next=JSON.stringify([state.mapLayers,state.feeds]);
    if(signature===next)return;signature=next;button.disabled=false;
    const active=[];
    for(const row of rows){
      const feed=state.feeds?.find(feed=>feed.kind===row.kind);
      row.input.checked=row.kind ? !!feed?.enabled : !!state.mapLayers?.[row.key];
      if(row.input.checked&&!row.display)active.push(row.name);
      row.status.textContent=row.kind ? row.source+(feed?.enabled ? ' · '+feed.count+' · '+feed.state : '') : '';
    }
    button.textContent='Layers · '+active.length+' active ▾';
    button.title=active.length ? active.join(', ') : 'No layers active';
    button.setAttribute('aria-label','Layers · '+active.length+' active'+(active.length ? ': '+active.join(', ') : ''));
    // A single short line keeps active names visible without filling the map.
    let summary=root.querySelector('.layers-summary');
    if(!summary){summary=document.createElement('span');summary.className='layers-summary';root.insertBefore(summary,panel);}
    summary.textContent=active.join(' · ')||'None active';
    summary.title=summary.textContent;
  };
}
