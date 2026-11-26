const designs = [
  {id:'a', title:'双灯信号', caption:'先看哪个会话需要你。'},
  {id:'b', title:'桌角探头', caption:'先认出工位上的那只宠物。'},
  {id:'c', title:'回合引号', caption:'把项目和对话排成一本索引。'},
];
const projects = [
  {name:'red-green', source:'Codex CLI', key:'8a0d3e17', state:'awaiting', label:'待确认'},
  {name:'ink-notes', source:'Claude Code', key:'f40019ce', state:'running', label:'运行中'},
  {name:'nomad-notes', source:'Codex CLI', key:'d70b36ac', state:'completed', label:'已完成'},
];
const conversation = [
  ['你','回看前面那段话时，新回复到了会怎么样？'],
  ['助手','你读到的那一段会留在原处。等你读完，点「有新消息」就能接着往下看。'],
  ['你','好，那我可以慢慢读完这一段。'],
  ['助手','可以。你读到哪里，下一屏就从哪里接起。'],
  ['你','看完后我想切去另一个项目。'],
  ['助手','点「会话列表」，再选你想看的项目。'],
];
const board = document.querySelector('#board');
const views = new Map();

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}
function button(label, action, className) {
  const node = element('button', className, label);
  node.type = 'button';
  node.addEventListener('click', action);
  return node;
}

// The same geometry is drawn in colour, as a light template, and as a dark template.
function mark(id, size, mode = 'colour') {
  const canvas = document.createElement('canvas');
  const ratio = Math.max(2, Math.ceil(window.devicePixelRatio || 1));
  canvas.width = canvas.height = size * ratio;
  canvas.style.width = canvas.style.height = size + 'px';
  canvas.setAttribute('aria-hidden','true');
  const g = canvas.getContext('2d');
  g.scale(canvas.width / 24, canvas.height / 24);
  const colour = mode === 'colour';
  const ink = mode === 'white' ? '#fff' : '#191e21';
  const path = (d, fill) => { g.fillStyle = fill; g.fill(new Path2D(d)); };
  const circle = (x,y,r,fill) => { g.beginPath(); g.arc(x,y,r,0,Math.PI*2); g.fillStyle=fill; g.fill(); };
  const cut = (draw) => { g.save(); g.globalCompositeOperation='destination-out'; draw(); g.restore(); };
  if (id === 'a') {
    g.translate(12,12); g.rotate(Math.PI/4);
    g.fillStyle = ink; g.beginPath(); g.roundRect(-6.65,-12,13.3,24,6.65); g.fill();
    if (colour) {
      circle(0,-5.15,4.35,'#ed685b'); circle(0,5.15,4.35,'#a4ecaf');
      circle(0,5.15,1.8,ink);
    } else {
      cut(() => { circle(0,-5.15,3.65,ink); circle(0,5.15,3.65,ink); });
      circle(0,5.15,1.45,ink);
    }
  } else if (id === 'b') {
    const face = 'M2 3h3v2h3v2h7V5h3V3h3v6h2v9h-2v2h-4v2H7v-2H3v-2H1V9h1z';
    path(face, ink);
    if (colour) {
      path('M4 7h2v2h11V8h2v3h2v5h-2v2h-3v2H8v-2H5v-2H3v-5h1z','#b3aabc');
      path('M4 5h1v2H4z M18 7h1V5h-1z','#ef7d60');
      path('M6 11h3v4H6z M15 11h3v4h-3z M10 16h4v2h-4z',ink);
    } else {
      cut(()=>path('M6 10.5h3v4H6z M15 10.5h3v4h-3z M10 16h4v2h-4z',ink));
    }
  } else {
    // Alternating speaker marks: a cut-out line in the first bubble, a full stop in the reply.
    path('M1 3h15v9H9l-5 4v-4H1z',colour ? '#912e26' : ink);
    cut(()=>path('M4 6h9v2.2H4z',ink));
    path('M10 14h8v-4h5v10h-3v4l-5-4h-5z',colour ? '#17534b' : ink);
    cut(()=>circle(18,16.65,1.35,ink));
  }
  return canvas;
}
function pet(project, scale = 4) {
  const canvas = document.createElement('canvas');
  canvas.setAttribute('aria-hidden','true');
  const frame = window.SPRITES.spec(window.SKINS.pick(project.name), project.state).frames[0];
  window.SPRITES.draw(canvas, frame, scale);
  return canvas;
}
function stateLabel(project) {
  const span = element('span','state s-' + project.state);
  span.append(element('i'),document.createTextNode(project.label));
  return span;
}
function brand(id, small=false) {
  const wrap = element('span','brand');
  wrap.append(mark(id,small ? 18 : 24),element('b','', 'red-green'));
  return wrap;
}
function specimen(d) {
  const hero = element('section','specimen');
  hero.setAttribute('aria-label', '应用图标与字标');
  const tile = element('span','tile'); tile.append(mark(d.id, 76));
  hero.append(tile,element('span','logotype','red-green'));
  const strip = element('div','strip');
  strip.append(element('span','strip-label','Mac 菜单栏'));
  for (const [theme,mode,label] of [['light','black','浅色菜单栏'],['dark','white','深色菜单栏']]) {
    const item = element('span','tray ' + theme);
    item.title = label + '，18 像素图标';
    item.append(mark(d.id,18,mode));
    strip.append(item);
  }
  return [hero,strip];
}
function makeRow(d, project) {
  const row = button('',()=>show(d, project, 'thread', true),'project');
  const name = element('strong','pname',project.name);
  const identity = element('span','pidentity');
  const meta = element('small','pmeta',project.source + '  /  ' + project.key);
  identity.append(name,meta);
  if (d.id === 'a') {
    const rail = element('span','rail s-' + project.state); rail.append(element('i'));
    row.append(rail,identity,stateLabel(project));
  } else if (d.id === 'b') {
    const character = element('span','character');
    character.append(pet(project,6));
    row.append(stateLabel(project),character,identity);
    meta.textContent = project.source;
    meta.append(element('span','short-id',project.key));
  } else {
    meta.textContent = project.key;
    row.append(identity,stateLabel(project));
  }
  row.setAttribute('aria-label',project.name + '，' + project.source + '，' + project.label + '，阅读对话');
  return row;
}
function listView(d) {
  const body = element('div','session-list');
  const header = element('div','list-title');
  const heading = element('h3','',d.id === 'a' ? '会话信号' : d.id === 'b' ? '桌面伙伴' : '项目索引');
  heading.tabIndex = -1;
  header.append(heading,element('span','count','3 个会话'));
  body.append(header);
  if (d.id === 'c') {
    for (const source of ['Claude Code','Codex CLI']) {
      const group = element('section','source-group');
      group.append(element('h4','',source));
      projects.filter(p=>p.source===source).forEach(p=>group.append(makeRow(d,p)));
      body.append(group);
    }
  } else {
    const rows = element('div','roster');
    projects.forEach(p=>rows.append(makeRow(d,p)));
    body.append(rows);
  }
  return body;
}
function threadView(d,p) {
  const body=element('div','thread');
  const back=button('‹ 会话列表',()=>show(d,p,'list',true),'back');
  const head=element('div','thread-head');
  const title=element('h3','',p.name); title.tabIndex=-1;
  if(d.id==='b') head.append(pet(p,5));
  head.append(title,stateLabel(p));
  const source=element('p','thread-source',p.source + '  /  ' + p.key);
  const text=element('div','transcript');
  const pages=element('div','pages');
  let page=0;
  const previous=button('上一屏',()=>draw(page-1));
  const next=button('下一屏',()=>draw(page+1));
  const position=element('span','position');
  position.setAttribute('aria-live','polite');
  pages.append(previous,position,next);
  function draw(index) {
    page=index;
    const lines=conversation.slice(page*3,page*3+3).map(([speaker,words])=>{
      const message=element('section','message ' + (speaker==='助手'?'reply':'ask'));
      message.append(element('h4','speaker',speaker),element('p','',words));
      return message;
    });
    text.replaceChildren(...lines);
    const count=Math.ceil(conversation.length/3);
    previous.disabled=page===0;
    next.disabled=page===count-1;
    position.textContent=(page+1) + ' / ' + count;
  }
  draw(0);
  body.append(back,head,source,text,pages);
  return body;
}
function show(d,p,kind,focus=false) {
  const screen=document.querySelector('#screen-' + d.id);
  views.set(d.id,{p,kind});
  const bar=element('div','reader-bar');
  bar.append(brand(d.id,true),element('span','page-kind','会话阅读'));
  screen.replaceChildren(bar,kind==='list'?listView(d):threadView(d,p));
  if(focus) screen.querySelector('h3').focus({preventScroll:true});
  for(const [sel,value] of [['#lists','list'],['#threads','thread']]){
    document.querySelector(sel).setAttribute('aria-pressed',String([...views.values()].every(v=>v.kind===value)));
  }
}
for(const d of designs){
  const card=element('article','direction ' + d.id);
  const title=element('header','direction-head');
  title.append(element('span','letter',d.id.toUpperCase()),element('h2','',d.title));
  const screen=element('section','screen'); screen.id='screen-' + d.id;
  screen.setAttribute('aria-label',d.title + '阅读器示意');
  card.append(title,element('p','caption',d.caption),...specimen(d),screen);
  board.append(card);
  show(d,projects[0],'list');
}
document.querySelector('#lists').onclick=()=>designs.forEach(d=>show(d,views.get(d.id).p,'list'));
document.querySelector('#threads').onclick=()=>designs.forEach(d=>show(d,projects[0],'thread'));
document.querySelector('#ink').onchange=e=>board.classList.toggle('ink',e.target.checked);
