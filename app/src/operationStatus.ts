import {useEffect,useRef,useSyncExternalStore} from 'react';
const active=new Set<symbol>();
const listeners=new Set<()=>void>();
function notify(){listeners.forEach(listener=>listener());}
export function useReportBusy(busy:boolean){
 const id=useRef(Symbol());
 useEffect(()=>{if(busy){active.add(id.current);notify();}return()=>{if(active.delete(id.current))notify();};},[busy]);
}
export function useOperationBusy(){return useSyncExternalStore(callback=>{listeners.add(callback);return()=>{listeners.delete(callback);};},()=>active.size>0,()=>false);}
