export function safeImageSrc(value) {
  const src = String(value || '').trim();
  // 缩略图只接受后端生成的 data:image URL，避免 WebView 把任意 http(s) 或 javascript: 字符串当成图片地址加载。
  // Thumbnails accept only backend-built data:image URLs so the WebView cannot load an arbitrary http(s) or javascript: string as an image.
  return src.startsWith('data:image/') ? src : '';
}
