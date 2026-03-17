import{M as i,j as e,aa as n,t as c,l as r,a5 as z,b as S}from"./index-8mC8-Qbi.js";/**
 * @license lucide-react v0.552.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const _=[["path",{d:"m15 18-6-6 6-6",key:"1wnfg3"}]],L=i("chevron-left",_);/**
 * @license lucide-react v0.552.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const R=[["path",{d:"m11 17-5-5 5-5",key:"13zhaf"}],["path",{d:"m18 17-5-5 5-5",key:"h8a8et"}]],F=i("chevrons-left",R);/**
 * @license lucide-react v0.552.0 - ISC
 *
 * This source code is licensed under the ISC license.
 * See the LICENSE file in the root directory of this source tree.
 */const M=[["path",{d:"m6 17 5-5-5-5",key:"xnjwq"}],["path",{d:"m13 17 5-5-5-5",key:"17xmmf"}]],I=i("chevrons-right",M);function P({page:s,pageSize:a,total:t,onPageChange:o,onPageSizeChange:u,borderPosition:d="top",itemLabel:x="rows",layout:j="default",showRange:g=!0,rangeFormat:y="slash"}){const l=Math.max(1,Math.ceil(t/a)),h=s>1,p=s<l,m=t===0?0:(s-1)*a+1,b=Math.min(s*a,t),N=[{label:"50/page",value:"50"},{label:"100/page",value:"100"},{label:"200/page",value:"200"},{label:"500/page",value:"500"}],$=d==="top"?"border-t":"border-b",k=d==="top"?"borderTopColor":"borderBottomColor",C=y==="of"?`${m}–${b} of ${t} ${x}`:`${m}–${b} / ${t} ${x}`,f=g?e.jsx("span",{style:{color:r.text.muted,fontSize:c.fontSize.xs},children:C}):null,v=e.jsxs("div",{className:"flex items-center gap-2",children:[e.jsx(n,{variant:"secondary",size:"xs",disabled:!h,onClick:()=>o(1),"aria-label":"First page",title:"First page",children:e.jsx(F,{className:"w-3 h-3"})}),e.jsx(n,{variant:"secondary",size:"xs",disabled:!h,onClick:()=>o(s-1),"aria-label":"Previous page",title:"Previous page",children:e.jsx(L,{className:"w-3 h-3"})}),e.jsxs("span",{style:{color:r.text.secondary,fontSize:c.fontSize.xs,minWidth:"80px",textAlign:"center"},children:["Page ",s," / ",l]}),e.jsx(n,{variant:"secondary",size:"xs",disabled:!p,onClick:()=>o(s+1),"aria-label":"Next page",title:"Next page",children:e.jsx(z,{className:"w-3 h-3"})}),e.jsx(n,{variant:"secondary",size:"xs",disabled:!p,onClick:()=>o(l),"aria-label":"Last page",title:"Last page",children:e.jsx(I,{className:"w-3 h-3"})}),e.jsx("div",{className:"ml-1",children:e.jsx(S,{value:a.toString(),onChange:w=>u(parseInt(w,10)),options:N,size:"sm"})})]});return e.jsx("div",{className:`flex items-center justify-between ${$} backdrop-blur-sm`,style:{padding:"6px 12px",backgroundColor:`${r.bg.surface}cc`,[k]:`${r.neutral[800]}66`,fontSize:c.fontSize.xs},children:j==="split"?e.jsxs(e.Fragment,{children:[f,v]}):e.jsxs(e.Fragment,{children:[v,f]})})}export{P};
