type Props = {maximized:boolean; onMinimize:()=>void; onMaximize:()=>void; onClose:()=>void};

// Windows-style client controls; actions go through the native window gateway.
export default function WindowControls({maximized,onMinimize,onMaximize,onClose}:Props){
  return <div className="caption-controls" aria-label="窗口操作">
    <button aria-label="最小化" title="最小化" onClick={onMinimize}><svg viewBox="0 0 12 12" aria-hidden="true"><path d="M1 6.5h10"/></svg></button>
    <button aria-label={maximized?'还原':'最大化'} title={maximized?'还原':'最大化'} onClick={onMaximize}><svg viewBox="0 0 12 12" aria-hidden="true">{maximized?<path d="M3.5 3.5v-2h7v7h-2m-7-5h7v7h-7z"/>:<rect x="1.5" y="1.5" width="9" height="9"/>}</svg></button>
    <button className="caption-close" aria-label="关闭窗口" title="关闭窗口" onClick={onClose}><svg viewBox="0 0 12 12" aria-hidden="true"><path d="m1.5 1.5 9 9m0-9-9 9"/></svg></button>
  </div>;
}
