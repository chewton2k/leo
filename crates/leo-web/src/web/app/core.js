const plural = (n, word, many = `${word}s`) => `${n} ${n === 1 ? word : many}`;
const md = window.leoMarkdown;
const leoDoc = window.leoDoc;
const felix = window.leoChat;
const noAi = felix.noAi;
const esc = md.escape;
const enc = encodeURIComponent;
const $ = (selector, root = document) => root.querySelector(selector);
const app = $('#app');
const floating = $('#floating');

const svg = (paths) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
const ON_MAC = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
const FOLDER = '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>';
const ICON = {
  back: svg('<path d="M15 18l-6-6 6-6"/>'),
  book: svg('<path d="M3 5.5A1.5 1.5 0 0 1 4.5 4H9a3 3 0 0 1 3 3v13a2.5 2.5 0 0 0-2.5-2.5h-5A1.5 1.5 0 0 1 3 16z"/><path d="M21 5.5A1.5 1.5 0 0 0 19.5 4H15a3 3 0 0 0-3 3v13a2.5 2.5 0 0 1 2.5-2.5h5A1.5 1.5 0 0 0 21 16z"/>'),
  pen: svg('<path d="M4 20l1.2-4.6L16.6 4a2 2 0 012.8 2.8L8 18.4z"/><path d="M14.5 6.1l3.4 3.4"/>'),
  search: svg('<circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/>'),
  list: svg('<path d="M9 6h11M9 12h11M9 18h11M4 6h.1M4 12h.1M4 18h.1"/>'),
  readThrough: svg('<path d="M4 7h9M4 12h9M4 17h6"/><circle class="dot" cx="18.5" cy="12" r="2.6"/>'),
  sort: svg('<path d="M4 6h16M4 12h11M4 18h6"/>'),
  close: svg('<path d="M6 6l12 12M18 6L6 18"/>'),
  more: svg('<circle cx="5" cy="12" r="1.3"/><circle cx="12" cy="12" r="1.3"/><circle cx="19" cy="12" r="1.3"/>'),
  folder: svg(FOLDER),
  folderPlus: svg('<path d="M11 19.5H6.5A3.5 3.5 0 0 1 3 16V8a3 3 0 0 1 3-3h2.9a2 2 0 0 1 1.5.7l1.2 1.4a2 2 0 0 0 1.5.7H18a3 3 0 0 1 3 3v1"/><circle cx="17.5" cy="17.5" r="4.6"/><path d="M17.5 15.2v4.6M15.2 17.5h4.6"/>'),
  move: svg(FOLDER + '<path d="M9.5 13.5h6M13 11l2.5 2.5L13 16"/>'),
  pin: svg('<path d="M12 16v6"/><path d="M8 3h8l-1.2 6.2L18 12.5V15H6v-2.5l3.2-3.3z"/>'),
  trash: svg('<path d="M4 7h16M10 11v6M14 11v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3"/>'),
  share: svg('<path d="M12 3v12M8 7l4-4 4 4"/><path d="M5 12v7a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-7"/>'),
  plus: svg('<path d="M12 5v14M5 12h14"/>'),
  download: svg('<path d="M12 4v12M7 11l5 5 5-5M4 18v3h16v-3"/>'),
  upload: svg('<path d="M12 16V4M7 9l5-5 5 5"/><path d="M4 16v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3"/>'),
  image: svg('<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5-5-9 9"/>'),
  paperclip: svg('<path d="M21 11l-8.5 8.5a5 5 0 0 1-7-7L14 4a3.5 3.5 0 0 1 5 5l-8.5 8.5a2 2 0 0 1-3-3L15 7"/>'),
  gear: svg('<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.6 1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.6 1.6 0 0 0 1.5-1 1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z"/>'),
  board: svg('<rect x="3.5" y="4" width="4.6" height="15" rx="1.3"/><rect x="9.7" y="4" width="4.6" height="9.5" rx="1.3"/><rect x="15.9" y="4" width="4.6" height="12" rx="1.3"/>'),
  calendar: svg('<rect x="4" y="5" width="16" height="15" rx="2"/><path d="M4 10h16M9 3v4M15 3v4"/>'),
  expand: svg('<path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7"/>'),
  restore: svg('<path d="M4 12a8 8 0 1 0 2.3-5.6L4 8.5"/><path d="M4 4v4.5h4.5"/>'),
  filePlus: svg('<path d="M11.5 21H8a4 4 0 0 1-4-4V7a4 4 0 0 1 4-4h5.8a3 3 0 0 1 2.1.9l1.9 1.9a3 3 0 0 1 .9 2.1v3.6"/><path d="M8 9h5M8 13h3"/><circle cx="17.5" cy="17.5" r="4.6"/><path d="M17.5 15.2v4.6M15.2 17.5h4.6"/>'),
  note: svg('<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/>'),
  refresh: svg('<path d="M20 12a8 8 0 1 1-2.3-5.6L20 8.5"/><path d="M20 4v4.5h-4.5"/>'),
  check: svg('<path d="M5 12l4.5 4.5L19 7"/>'),
  chevron: svg('<path d="M9 6l6 6-6 6"/>'),
  lock: svg('<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>'),
  map: svg('<circle cx="6" cy="7" r="2.2"/><circle cx="18" cy="6" r="2.2"/><circle cx="12" cy="17.5" r="2.2"/><path d="M7.4 8.9l3.5 6.7M16.9 7.9l-3.8 7.8M8.2 6.8l7.6-.6"/>'),
  access: svg('<circle cx="12" cy="12" r="9"/><circle cx="12" cy="7.2" r="1.3"/><path d="M7.5 10.2l4.5 1 4.5-1M12 11.2v3.6l-2.2 3.7M12 14.8l2.2 3.7"/>'),
  mic: svg('<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>'),
  screen: svg('<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>'),
  sidebar: svg('<rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M9 4v16"/>'),
  send: svg('<path d="M12 19V5M6 11l6-6 6 6"/>'),
  chat: svg('<path d="M5 18.5V7a2.5 2.5 0 0 1 2.5-2.5h9A2.5 2.5 0 0 1 19 7v6.5a2.5 2.5 0 0 1-2.5 2.5H9z"/><path d="M9 9.5h6M9 12.5h4"/>'),
  storage: svg('<ellipse cx="12" cy="6" rx="7.5" ry="2.8"/><path d="M4.5 6v6c0 1.5 3.4 2.8 7.5 2.8s7.5-1.3 7.5-2.8V6"/><path d="M4.5 12v6c0 1.5 3.4 2.8 7.5 2.8s7.5-1.3 7.5-2.8v-6"/>'),
  logoOllama: '<svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true"><path d="M16.361 10.26a.894.894 0 0 0-.558.47l-.072.148.001.207c0 .193.004.217.059.353.076.193.152.312.291.448.24.238.51.3.872.205a.86.86 0 0 0 .517-.436.752.752 0 0 0 .08-.498c-.064-.453-.33-.782-.724-.897a1.06 1.06 0 0 0-.466 0zm-9.203.005c-.305.096-.533.32-.65.639a1.187 1.187 0 0 0-.06.52c.057.309.31.59.598.667.362.095.632.033.872-.205.14-.136.215-.255.291-.448.055-.136.059-.16.059-.353l.001-.207-.072-.148a.894.894 0 0 0-.565-.472 1.02 1.02 0 0 0-.474.007Zm4.184 2c-.131.071-.223.25-.195.383.031.143.157.288.353.407.105.063.112.072.117.136.004.038-.01.146-.029.243-.02.094-.036.194-.036.222.002.074.07.195.143.253.064.052.076.054.255.059.164.005.198.001.264-.03.169-.082.212-.234.15-.525-.052-.243-.042-.28.087-.355.137-.08.281-.219.324-.314a.365.365 0 0 0-.175-.48.394.394 0 0 0-.181-.033c-.126 0-.207.03-.355.124l-.085.053-.053-.032c-.219-.13-.259-.145-.391-.143a.396.396 0 0 0-.193.032zm.39-2.195c-.373.036-.475.05-.654.086-.291.06-.68.195-.951.328-.94.46-1.589 1.226-1.787 2.114-.04.176-.045.234-.045.53 0 .294.005.357.043.524.264 1.16 1.332 2.017 2.714 2.173.3.033 1.596.033 1.896 0 1.11-.125 2.064-.727 2.493-1.571.114-.226.169-.372.22-.602.039-.167.044-.23.044-.523 0-.297-.005-.355-.045-.531-.288-1.29-1.539-2.304-3.072-2.497a6.873 6.873 0 0 0-.855-.031zm.645.937a3.283 3.283 0 0 1 1.44.514c.223.148.537.458.671.662.166.251.26.508.303.82.02.143.01.251-.043.482-.08.345-.332.705-.672.957a3.115 3.115 0 0 1-.689.348c-.382.122-.632.144-1.525.138-.582-.006-.686-.01-.853-.042-.57-.107-1.022-.334-1.35-.68-.264-.28-.385-.535-.45-.946-.03-.192.025-.509.137-.776.136-.326.488-.73.836-.963.403-.269.934-.46 1.422-.512.187-.02.586-.02.773-.002zm-5.503-11a1.653 1.653 0 0 0-.683.298C5.617.74 5.173 1.666 4.985 2.819c-.07.436-.119 1.04-.119 1.503 0 .544.064 1.24.155 1.721.02.107.031.202.023.208a8.12 8.12 0 0 1-.187.152 5.324 5.324 0 0 0-.949 1.02 5.49 5.49 0 0 0-.94 2.339 6.625 6.625 0 0 0-.023 1.357c.091.78.325 1.438.727 2.04l.13.195-.037.064c-.269.452-.498 1.105-.605 1.732-.084.496-.095.629-.095 1.294 0 .67.009.803.088 1.266.095.555.288 1.143.503 1.534.071.128.243.393.264.407.007.003-.014.067-.046.141a7.405 7.405 0 0 0-.548 1.873c-.062.417-.071.552-.071.991 0 .56.031.832.148 1.279L3.42 24h1.478l-.05-.091c-.297-.552-.325-1.575-.068-2.597.117-.472.25-.819.498-1.296l.148-.29v-.177c0-.165-.003-.184-.057-.293a.915.915 0 0 0-.194-.25 1.74 1.74 0 0 1-.385-.543c-.424-.92-.506-2.286-.208-3.451.124-.486.329-.918.544-1.154a.787.787 0 0 0 .223-.531c0-.195-.07-.355-.224-.522a3.136 3.136 0 0 1-.817-1.729c-.14-.96.114-2.005.69-2.834.563-.814 1.353-1.336 2.237-1.475.199-.033.57-.028.776.01.226.04.367.028.512-.041.179-.085.268-.19.374-.431.093-.215.165-.333.36-.576.234-.29.46-.489.822-.729.413-.27.884-.467 1.352-.561.17-.035.25-.04.569-.04.319 0 .398.005.569.04a4.07 4.07 0 0 1 1.914.997c.117.109.398.457.488.602.034.057.095.177.132.267.105.241.195.346.374.43.14.068.286.082.503.045.343-.058.607-.053.943.016 1.144.23 2.14 1.173 2.581 2.437.385 1.108.276 2.267-.296 3.153-.097.15-.193.27-.333.419-.301.322-.301.722-.001 1.053.493.539.801 1.866.708 3.036-.062.772-.26 1.463-.533 1.854a2.096 2.096 0 0 1-.224.258.916.916 0 0 0-.194.25c-.054.109-.057.128-.057.293v.178l.148.29c.248.476.38.823.498 1.295.253 1.008.231 2.01-.059 2.581a.845.845 0 0 0-.044.098c0 .006.329.009.732.009h.73l.02-.074.036-.134c.019-.076.057-.3.088-.516.029-.217.029-1.016 0-1.258-.11-.875-.295-1.57-.597-2.226-.032-.074-.053-.138-.046-.141.008-.005.057-.074.108-.152.376-.569.607-1.284.724-2.228.031-.26.031-1.378 0-1.628-.083-.645-.182-1.082-.348-1.525a6.083 6.083 0 0 0-.329-.7l-.038-.064.131-.194c.402-.604.636-1.262.727-2.04a6.625 6.625 0 0 0-.024-1.358 5.512 5.512 0 0 0-.939-2.339 5.325 5.325 0 0 0-.95-1.02 8.097 8.097 0 0 1-.186-.152.692.692 0 0 1 .023-.208c.208-1.087.201-2.443-.017-3.503-.19-.924-.535-1.658-.98-2.082-.354-.338-.716-.482-1.15-.455-.996.059-1.8 1.205-2.116 3.01a6.805 6.805 0 0 0-.097.726c0 .036-.007.066-.015.066a.96.96 0 0 1-.149-.078A4.857 4.857 0 0 0 12 3.03c-.832 0-1.687.243-2.456.698a.958.958 0 0 1-.148.078c-.008 0-.015-.03-.015-.066a6.71 6.71 0 0 0-.097-.725C8.997 1.392 8.337.319 7.46.048a2.096 2.096 0 0 0-.585-.041Zm.293 1.402c.248.197.523.759.682 1.388.03.113.06.244.069.292.007.047.026.152.041.233.067.365.098.76.102 1.24l.002.475-.12.175-.118.178h-.278c-.324 0-.646.041-.954.124l-.238.06c-.033.007-.038-.003-.057-.144a8.438 8.438 0 0 1 .016-2.323c.124-.788.413-1.501.696-1.711.067-.05.079-.049.157.013zm9.825-.012c.17.126.358.46.498.888.28.854.36 2.028.212 3.145-.019.14-.024.151-.057.144l-.238-.06a3.693 3.693 0 0 0-.954-.124h-.278l-.119-.178-.119-.175.002-.474c.004-.669.066-1.19.214-1.772.157-.623.434-1.185.68-1.382.078-.062.09-.063.159-.012z"/></svg>',
  logoOpenai: '<svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true"><path d="M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z"/></svg>',
  logoClaude: '<svg viewBox="0 0 24 24" fill="#D97757" stroke="none" aria-hidden="true"><path d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z"/></svg>',
};
const lineDrawing = (name, box, paths) => `<svg class="drawn drawn-${name}" viewBox="${box}" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;
const DRAWN = {
  trash: lineDrawing('trash', '0 0 120 92', '<path d="M37 31 Q39 57 41.5 82 Q60.5 83.4 79.5 82 Q82.4 57 84.6 30.4"/><path d="M31 29.23 C35.92 29.32 50.67 29.72 60.5 29.77 C70.33 29.83 85.08 29.6 90 29.56"/><path d="M51.5 40 Q52.2 60 52.8 73 M69 40 Q68.7 58 68.2 73" stroke-width="1.1" opacity=".55"/><path d="M52.5 28.5 Q53 22.5 60.5 22.3 Q68 22.4 68.6 28.6" stroke-width="1.3"/>'),
  board: lineDrawing('board', '0 0 120 92', '<path d="M31 22.6 Q30.6 49 31.3 76.4 Q37.4 76.8 43.6 76.2 Q44.1 49 43.5 22.2 Q37.2 22.5 31 22.6"/><path d="M53.4 22.3 Q53.1 41 53.6 58.5 Q60 58.9 66.4 58.3 Q66.8 40 66.2 22.1 Q59.7 22.6 53.4 22.3"/><path d="M76.2 22.5 Q76 45 76.5 67.6 Q83 68 89.2 67.4 Q89.5 44 89 22.2 Q82.6 22.6 76.2 22.5"/><path d="M56.5 29.5 Q60 29.2 63.4 29.6 M34.2 29.8 Q37.5 29.5 40.6 29.9 M34.3 36.4 Q37 36.2 39.6 36.5 M79.3 29.7 Q82.6 29.4 86 29.8" stroke-width="1.1" opacity=".55"/>'),
  tasks: lineDrawing('tasks', '32 20 56 58', '<path d="M38 27.4 Q42.6 27.2 47.3 27.6 Q47.6 32 47.2 36.6 Q42.6 36.9 37.9 36.5 Q37.6 32 38 27.4"/><path d="M39.8 31.8 Q41.2 33 42.3 35.1 Q44.2 30.4 49.4 25.6"/><path d="M38.1 45.3 Q42.7 45.1 47.2 45.5 Q47.5 50 47.1 54.4 Q42.5 54.7 37.8 54.4 Q37.5 49.8 38.1 45.3"/><path d="M38 63.2 Q42.6 63 47.1 63.4 Q47.4 67.9 47 72.3 Q42.4 72.6 37.9 72.2 Q37.6 67.7 38 63.2"/><path d="M55.5 32.2 Q68 31.6 82.3 32.4 M55.4 50.1 Q66 49.6 76.4 50.3 M55.6 67.9 Q69 67.5 84.1 68.2" stroke-width="1.1" opacity=".55"/>'),
  chats: lineDrawing('chats', '0 0 120 92', '<path d="M34.5 26.2 Q60 24.6 85.4 26.4 Q87.6 41 85.2 55.6 Q70 57 54.6 56.1 Q47.3 62.9 40.8 67.4 Q42.3 61.5 43.1 56 Q38.6 55.8 34.8 55.4 Q32.8 41 34.5 26.2"/><path d="M44.2 36.8 Q60 36.2 75.6 36.9 M44.3 45.6 Q55 45.2 66.2 45.8" stroke-width="1.1" opacity=".55"/>'),
  search: lineDrawing('search', '0 0 120 92', '<path d="M54.2 22.6 Q67.8 22.1 69.6 36.4 Q70.4 50.2 55.4 51.4 Q41.6 51.8 40.2 37.6 Q39.4 23.4 54.2 22.6"/><path d="M66.3 47.6 Q73.4 56.4 80.6 66.2"/><path d="M48.4 32.6 Q50.6 29.2 54.6 28.9" stroke-width="1.1" opacity=".55"/>'),
  arrow: lineDrawing('arrow', '0 0 190 150', '<path d="M184 140 C178.33 138.67 164.67 138 150 132 C135.33 126 112.33 115.67 96 104 C79.67 92.33 65.33 75.67 52 62 C38.67 48.33 22 28.67 16 22 M24.49 24.98 Q20.65 23.49 16 22 Q17.26 26.72 18.51 30.64" stroke-width="1.4"/>'),
  offline: lineDrawing('offline', '0 0 120 92', '<path d="M76.2 30.2 Q86 29.6 92.4 30.8 Q93.6 46 92.2 61.2 Q84 62.2 76.4 61.6 Q75.2 46 76.2 30.2"/><path d="M82 41.2 Q82.3 44 82 47.4 M86.8 41 Q87 44 86.7 47.2" stroke-width="1.1" opacity=".55"/><path d="M48 36.4 Q55 36 60.2 36.8 Q61 46 60.4 55.4 Q54 56 48.2 55.6 Q47.4 46 48 36.4"/><path d="M60.6 41.8 Q64.4 41.6 67.4 42 M60.6 50.2 Q64.2 50 67.2 50.4"/><path d="M48 46 Q38.6 46.4 33.6 52 Q28.2 58.6 31.4 66 Q34.4 72.6 25.6 77.4"/>'),
  noink: lineDrawing('noink', '0 0 120 92', '<path d="M66.4 55.2 Q76 42 86 28.6 Q89.6 24.2 93 27.4 Q95.8 30.6 92.6 34 Q82.6 46.4 72.4 59.2"/><path d="M66.4 55.2 Q64.6 58.6 63.8 62.6 Q68 61.4 72.4 59.2"/><path d="M24 70 Q30 60.6 36 66.4 Q41.4 71.8 46.4 64.6 Q50 59.6 53.4 64"/><path d="M84.4 35.6 Q86.8 37.6 89 39.6 M56.8 66.2 Q58.2 67.2 59.6 66.6 M62 64.8 Q62.6 64.2 63.2 63.8" stroke-width="1.1" opacity=".55"/>'),
  broken: lineDrawing('broken', '0 0 120 92', '<g transform="rotate(-24 66 44)"><path d="M50 37.4 Q71 36.8 90.6 37.6 M50.2 52.2 Q71 52.6 90.4 51.8"/><path d="M90.6 37.6 Q97.4 37.8 97.8 44.8 Q97.4 51.6 90.4 51.8"/><path d="M50 37.4 Q44.6 40.4 40.8 42.2 Q42.4 43.6 40.6 45.2 Q42.4 46.4 40.6 47.8 Q44.8 49.6 50.2 52.2"/><path d="M85.4 37.8 Q85.8 44.8 85.4 51.8 M81.2 37.8 Q81.6 44.8 81.2 51.8 M52.6 44.8 Q66 45.1 78.6 44.8" stroke-width="1.1" opacity=".55"/></g><path d="M27.6 72.4 Q32.6 67.6 37.6 66 Q37.4 70.6 36 74.4 Q31.6 73.8 27.6 72.4" stroke-width="1.3"/><path d="M42.4 74.6 Q42.7 74.4 43 74.6 M46 71.4 Q46.3 71.2 46.6 71.4" stroke-width="1.1" opacity=".55"/>'),
};

const recorder = window.leoRecording.create({ bytes: speechBytes, encodePicture: async (file) => base64((await pictureFor(file)).blob), api, esc, toast, go: (hash, opts) => go(hash, opts), noteHash: (id) => noteHash(id), felix, icons: ICON, noteFields: (options) => noteFields(options), choiceCards: (options) => choiceCards(options), syncChoiceCards: (box) => syncChoiceCards(box), settingsSwitch: (attrs, on, label) => settingsSwitch(attrs, on, label), desktop: () => desktopApp, noteReady: () => showLatest().catch(() => {}) });
const chat = felix.create({
  render: (text, options) => md.render(text, options),
  escape: md.escape,
  icons: ICON,
  drawings: DRAWN,
  onOpen: (id, lines) => {
    citedLines = lines ? { id, ...lines } : null;
    if (state.view === 'note' && state.session && state.session.note.id === id) showCitedLines();
    else go(noteHash(id));
  },
  onCite: (id, lines) => {
    const open = state.view === 'note' ? state.session : null;
    if (open && open.doc) open.doc.glow(...(id && lines && open.note.id === id ? [lines.from, lines.to] : []));
  },
  onOriginal: (note, name, at) => showPassage(note, name, at).catch(fail),
  prepare: async (file) => {
    const ready = await shrink(file);
    return { name: ready.name, type: ready.type, blob: ready.blob };
  },
  notify: (message) => toast(message, { bad: true }),
  onLocked: () => showLocked(),
  onUndone: (note, removed) => {
    const open = state.view === 'note' && state.session && state.session.note.id === note.id;
    if (open && removed) go(folderHash(state.dir || ''), { replace: true });
    else if (open && !saving.unsaved()) showNote(note.id);
    else showLatest().catch(() => {});
    toast(removed ? `Moved “${note.title}” to the trash` : `Undid the change to “${note.title}”`);
  },
  onChanged: (note) => {
    const open = state.view === 'note' && state.session && state.session.note.id === note.id;
    if (open && !saving.unsaved()) showNote(note.id);
    if (tasksAtHome()) loadHomeRecent();
    toast(open && saving.unsaved() ? 'Felix changed this note while you were typing; reopen it to see the change.' : `Changed “${note.title}”`, { action: 'Open', run: () => go(noteHash(note.id)) });
  },
  onToggle: () => { drawSide(); pageBarLater(); marginRedraw(); },
  onOrganized: (proposal) => {
    const made = proposal.state === 'applied' && FELIX_MAKES[proposal.kind];
    if (made && proposal.undo?.[made]) inkLand(proposal.undo[made], true);
    if (MARGIN_KINDS.includes(proposal.kind)) {
      if (state.view === 'note' && state.session && state.session.note.id === proposal.note) marginLoad(state.session).catch(fail);
      return;
    }
    if (BOARD_KINDS.includes(proposal.kind)) {
      if (state.view === 'boards') showBoards((kanban.board && kanban.board.id) || '').catch(fail);
      return;
    }
    loadSideFolders().catch(() => {});
    if (state.view === 'calendar') loadSchedule(seq);
    loadTasksWindow().catch(() => {});
    if (state.view === 'note' && state.session && state.session.note.id && !saving.unsaved()) showNote(state.session.note.id).catch(() => go(folderHash(state.dir || ''), { replace: true }));
    else if (state.view !== 'calendar') showLatest().catch(() => {});
  },
  onAdd: (picked) => {
    if (!window.matchMedia('(min-width: 900px)').matches) chat.toggle(false);
    addSheet(picked);
  },
  onSaved: (note) => {
    showLatest().catch(() => {});
    toast(`Saved as a note${note.directory ? ` in ${note.directory}` : ''}`, { action: 'Open', run: () => go(noteHash(note.id)) });
  },
});
$('#chat-toggle').innerHTML = chat.button(36);
$('#search-toggle').innerHTML = ICON.search;
$('#menu').innerHTML = ICON.more;
$('#search-icon').innerHTML = ICON.search;

const pageBar = document.createElement('div');
pageBar.className = 'page-bar';
pageBar.hidden = true;
pageBar.dataset.keepSelection = '';
pageBar.setAttribute('aria-hidden', 'true');
pageBar.innerHTML = '<div class="page-bar-thumb"></div>';
document.body.appendChild(pageBar);
const pageThumb = pageBar.firstChild;
let pageBarFrame = 0;
let pageBarSize = { track: 0, thumb: 0, top: 0, room: 0 };

function pageBarRight() {
  const panels = [document.body.classList.contains('chat-open') && document.querySelector('aside.chat'), document.querySelector('.source-pane')]
    .filter(panel => panel && !panel.hidden && panel.getClientRects().length);
  if (!panels.length) return 0;
  const left = Math.min(...panels.map(panel => panel.offsetLeft));
  return left < 240 ? 0 : Math.max(0, document.documentElement.clientWidth - left);
}

function drawPageBar() {
  pageBarFrame = 0;
  const right = pageBarRight();
  const root = document.documentElement;
  root.classList.toggle('page-bar-on', right > 0);
  const room = root.scrollHeight - innerHeight;
  if (!right || room < 1) {
    pageBar.hidden = true;
    return;
  }
  pageBar.style.right = `${right}px`;
  pageBar.hidden = false;
  const track = pageBar.clientHeight;
  const thumb = Math.min(track, Math.max(36, track * innerHeight / root.scrollHeight));
  const top = (track - thumb) * Math.min(1, Math.max(0, scrollY / room));
  pageThumb.style.height = `${thumb}px`;
  pageThumb.style.transform = `translateY(${top}px)`;
  pageBarSize = { track, thumb, top, room };
}

function pageBarLater() {
  if (!pageBarFrame) pageBarFrame = requestAnimationFrame(drawPageBar);
}

pageThumb.addEventListener('pointerdown', (e) => {
  if (e.button !== 0) return;
  e.preventDefault();
  pageThumb.setPointerCapture(e.pointerId);
  pageBar.classList.add('dragging');
  const startY = e.clientY;
  const startScroll = scrollY;
  const { track, thumb, room } = pageBarSize;
  const move = (m) => {
    if (!(m.buttons & 1)) return stop();
    scrollTo(0, startScroll + (m.clientY - startY) * room / Math.max(1, track - thumb));
  };
  const stop = () => {
    pageBar.classList.remove('dragging');
    pageThumb.removeEventListener('pointermove', move);
    pageThumb.removeEventListener('pointerup', stop);
    pageThumb.removeEventListener('pointercancel', stop);
    pageThumb.removeEventListener('lostpointercapture', stop);
  };
  pageThumb.addEventListener('pointermove', move);
  pageThumb.addEventListener('pointerup', stop);
  pageThumb.addEventListener('pointercancel', stop);
  pageThumb.addEventListener('lostpointercapture', stop);
});
pageBar.addEventListener('pointerdown', (e) => {
  if (e.target !== pageBar || e.button !== 0) return;
  const y = e.clientY - pageBar.getBoundingClientRect().top;
  scrollBy(0, (y < pageBarSize.top ? -1 : 1) * innerHeight * 0.9);
});
let pageBarIdle = 0;
addEventListener('scroll', () => {
  pageBarLater();
  pageBar.classList.add('active');
  clearTimeout(pageBarIdle);
  pageBarIdle = setTimeout(() => pageBar.classList.remove('active'), 900);
}, { passive: true });
addEventListener('resize', pageBarLater);
if (window.ResizeObserver) new ResizeObserver(pageBarLater).observe(document.body);
new MutationObserver(pageBarLater).observe(document.body, { attributes: true, attributeFilter: ['class'] });
new MutationObserver(pageBarLater).observe(document.documentElement, { attributes: true, attributeFilter: ['style'] });
document.addEventListener('animationend', pageBarLater, true);

class Offline extends Error {}
class Locked extends Error {}

async function api(path, { method = 'GET', body, total = false, signal } = {}) {
  let response;
  try {
    response = await fetch(path, {
      method,
      credentials: 'same-origin',
      signal,
      headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch (e) {
    throw new Offline();
  }
  if (response.status === 401) throw new Locked();
  if (response.status === 204) return null;
  if (!response.ok) {
    let said = '';
    let data = null;
    try {
      data = await response.json();
      said = (data && data.error) || '';
    } catch (e) {
      said = '';
    }
    const error = new Error(said || (response.status === 404 ? 'That is not there any more.' : response.status === 400 ? 'Use a folder name inside your notes, without . or ..' : `leo answered ${response.status}.`));
    error.status = response.status;
    error.data = data;
    throw error;
  }
  const text = await response.text();
  const data = text.trim() ? JSON.parse(text) : null;
  if (total) return { items: data || [], total: Number(response.headers.get('x-total')) || 0 };
  return data;
}

function fail(error) {
  if (error instanceof Locked) return showLocked();
  if (error instanceof Offline) return toast("Can't reach leo. Quit leo and open it again.", { bad: true });
  toast(error.message || String(error), { bad: true });
}

let toastTimer;
function toast(message, { action, run, bad } = {}) {
  clearTimeout(toastTimer);
  const old = $('.toast');
  if (old) old.remove();
  const el = document.createElement('div');
  el.className = 'toast' + (bad ? ' bad' : '');
  el.setAttribute('role', 'status');
  const lasts = action ? 10000 : 3500;
  el.innerHTML = `<span>${esc(message)}</span>${action ? `<button>${esc(action)}</button><i class="toast-time" aria-hidden="true" style="animation-duration:${lasts}ms"></i>` : ''}`;
  if (action) {
    el.querySelector('button').addEventListener('click', () => {
      el.remove();
      run();
    });
  }
  document.body.appendChild(el);
  toastTimer = setTimeout(() => {
    el.classList.replace('toast', 'toast-gone');
    el.setAttribute('aria-hidden', 'true');
    el.inert = true;
    setTimeout(() => el.remove(), LEAVE_MS);
  }, lasts);
}

const LEAVE_MS = 160;
const MOTION_MS = 180;
const OPEN_MS = 200;
const DRAWER_MS = 240;
const PULSE_MS = 320;
const ICON_MS = 140;
const EXIT_SHARE = .8;
const POP_FROM = .95;
const POP_OPEN = 'inset(-24px)';
const CASCADE_MS = 30 * 5 + MOTION_MS + 30;
const EASE_OUT = 'cubic-bezier(.2, .8, .2, 1)';
const EASE_MOVE = 'cubic-bezier(.45, 0, .15, 1)';
const EASE_EXIT = 'cubic-bezier(.4, 0, 1, 1)';

function stillMotion() {
  return matchMedia('(prefers-reduced-motion: reduce)').matches;
}

const moving = new WeakMap();
const popEdges = new WeakMap();

function stopSlide(el) {
  delete el.dataset.folding;
  const run = moving.get(el);
  if (!run) return;
  moving.delete(el);
  run.onfinish = null;
  run.oncancel = null;
  run.cancel();
}

function pulse(el, { lift = 1.04, ring = false } = {}) {
  if (!el || stillMotion() || typeof el.animate !== 'function') return null;
  const frames = ring
    ? { boxShadow: ['0 0 0 0 transparent', '0 0 0 3px color-mix(in srgb, var(--focus) 45%, transparent)', '0 0 0 5px transparent'] }
    : { scale: [1, lift, 1] };
  return el.animate(frames, { duration: PULSE_MS, easing: EASE_MOVE });
}

const INK_MS = 600;
const FELIX_MAKES = { add_card: 'card', add_task: 'id', add_margin_note: 'id' };
const inkedAt = new Map();

function inkLand(id, felix = false) {
  if (!stillMotion()) inkedAt.set(id, { at: performance.now(), felix });
}

function inkSettle(find) {
  for (const [id, { at, felix }] of inkedAt) {
    const elapsed = performance.now() - at;
    if (elapsed >= INK_MS) { inkedAt.delete(id); continue; }
    for (const el of find(id)) {
      if (typeof el.animate !== 'function' || el.getAnimations().some((run) => run.id === 'ink')) continue;
      const look = getComputedStyle(el);
      const frames = felix ? { color: ['var(--felix)', look.color] } : { color: ['var(--text)', look.color], backgroundColor: [`color-mix(in srgb, var(--text) 12%, ${look.backgroundColor})`, look.backgroundColor] };
      el.animate(frames, { id: 'ink', duration: INK_MS, delay: -elapsed, easing: EASE_OUT });
    }
  }
}

function cascade(el, kind = 'rows-in') {
  if (!el || stillMotion()) return;
  el.classList.add(kind);
  setTimeout(() => el.classList.remove(kind), CASCADE_MS);
}

function rubber(over, reach = 24) {
  return Math.sign(over) * reach * (1 - 1 / (1 + Math.abs(over) / reach));
}

const pastEdge = (at, low, high) => (stillMotion() ? at : at < low ? low + rubber(at - low) : at > high ? high + rubber(at - high) : at);

function iconSwap(host, change, part = 'svg') {
  const old = host?.querySelector(part);
  if (!old || stillMotion() || typeof old.animate !== 'function') return change();
  const box = old.getBoundingClientRect();
  const ghost = old.cloneNode(true);
  ghost.setAttribute('aria-hidden', 'true');
  Object.assign(ghost.style, { position: 'fixed', zIndex: 1000, left: `${box.left}px`, top: `${box.top}px`, width: `${box.width}px`, height: `${box.height}px`, margin: 0, pointerEvents: 'none', color: getComputedStyle(old).color });
  document.body.appendChild(ghost);
  const result = change();
  const out = { opacity: [1, 0], filter: ['blur(0)', 'blur(2px)'], scale: [1, .85] };
  ghost.animate(out, { duration: ICON_MS * EXIT_SHARE, easing: EASE_EXIT, fill: 'forwards' }).onfinish = () => ghost.remove();
  host.querySelector(part)?.animate({ opacity: [0, 1], filter: ['blur(2px)', 'blur(0)'], scale: [.85, 1] }, { duration: ICON_MS, easing: EASE_OUT });
  return result;
}

function popEdge(menu, from) {
  const box = menu.getBoundingClientRect();
  const at = from.getBoundingClientRect ? from.getBoundingClientRect() : { left: from.x, right: from.x, top: from.y, bottom: from.y };
  const x = Math.max(0, Math.min(box.width, (at.left + at.right) / 2 - box.left));
  const y = Math.max(0, Math.min(box.height, (at.top + at.bottom) / 2 - box.top));
  return { origin: `${x}px ${y}px`, up: y > box.height / 2 };
}

const popShut = (edge) => (edge.up ? 'inset(100% 0 0 0)' : 'inset(0 0 100% 0)');

function popNow(menu) {
  if (!moving.has(menu)) return null;
  const style = getComputedStyle(menu);
  return { clipPath: style.clipPath, opacity: style.opacity, transform: style.transform };
}

function unfold(menu, from = null) {
  const now = popNow(menu);
  stopSlide(menu);
  menu.hidden = false;
  if (stillMotion()) return;
  const edge = from ? popEdge(menu, from) : { origin: '50% 0', up: false };
  popEdges.set(menu, edge);
  menu.style.transformOrigin = edge.origin;
  const run = menu.animate([now || { clipPath: popShut(edge), opacity: 1, transform: `scale(${POP_FROM})` }, { clipPath: POP_OPEN, opacity: 1, transform: 'none' }], { duration: MOTION_MS, easing: EASE_OUT });
  moving.set(menu, run);
  run.onfinish = () => moving.delete(menu);
}

function fold(menu, then = null) {
  if (menu.hidden || menu.dataset.folding) {
    if (then && menu.hidden) then();
    return;
  }
  const now = popNow(menu);
  stopSlide(menu);
  if (stillMotion()) {
    menu.hidden = true;
    if (then) then();
    return;
  }
  menu.dataset.folding = '1';
  const edge = popEdges.get(menu) || { up: false };
  const run = menu.animate([now || { clipPath: POP_OPEN, opacity: 1, transform: 'none' }, { clipPath: popShut(edge), opacity: 0, transform: `scale(${POP_FROM})` }], { duration: MOTION_MS * EXIT_SHARE, easing: EASE_EXIT });
  moving.set(menu, run);
  run.onfinish = () => {
    moving.delete(menu);
    delete menu.dataset.folding;
    menu.hidden = true;
    if (then) then();
  };
  run.oncancel = () => delete menu.dataset.folding;
}

function slideHeight(el, from, to, then) {
  stopSlide(el);
  if (stillMotion() || Math.abs(from - to) < 1) {
    if (then) then();
    return;
  }
  const leaving = to < from;
  const run = el.animate([{ height: `${from}px`, overflow: 'hidden' }, { height: `${to}px`, overflow: 'hidden' }], { duration: leaving ? MOTION_MS * EXIT_SHARE : MOTION_MS, easing: leaving ? EASE_EXIT : EASE_OUT });
  moving.set(el, run);
  run.onfinish = () => {
    moving.delete(el);
    if (then) then();
  };
}

function rel(iso) {
  const then = new Date(iso);
  const seconds = (Date.now() - then) / 1000;
  if (seconds < 60) return 'just now';
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  if (seconds < 2 * 86400) return 'yesterday';
  if (seconds < 7 * 86400) return then.toLocaleDateString(undefined, { weekday: 'long' });
  const sameYear = then.getFullYear() === new Date().getFullYear();
  return then.toLocaleDateString(undefined, sameYear ? { month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric', year: 'numeric' });
}

const folderLabel = (dir) => (dir ? dir.split('/').pop() : 'Top level');

function progress(note) {
  let done;
  let all;
  if (note.tasks) {
    [done, all] = note.tasks;
  } else {
    const boxes = String(note.body || '').match(/^\s*- \[( |x|X)\] /gm) || [];
    all = boxes.length;
    done = boxes.filter((b) => !b.includes('[ ]')).length;
  }
  if (!all) return '';
  return `<span class="chip progress">${ICON.check.replace('<svg', '<svg width="14" height="14"')} ${done}/${all}</span>`;
}

function highlight(text, words) {
  if (!words.length) return esc(text);
  const pattern = new RegExp(`(${words.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|')})`, 'gi');
  return text
    .split(pattern)
    .map((part, i) => (i % 2 ? `<mark>${esc(part)}</mark>` : esc(part)))
    .join('');
}

function snippet(body, words) {
  const text = md.plain(body);
  if (!text) return '';
  if (!words.length) return esc(text.slice(0, 180));
  const lower = text.toLowerCase();
  const at = Math.min(...words.map((w) => lower.indexOf(w.toLowerCase())).filter((i) => i >= 0), Infinity);
  if (at === Infinity) return esc(text.slice(0, 180));
  const start = Math.max(0, at - 50);
  return (start ? '…' : '') + highlight(text.slice(start, start + 180), words);
}

function pinButton(note) {
  const label = note.pinned ? 'Unpin' : 'Pin to the top';
  return `<button class="pin-toggle${note.pinned ? ' on' : ''}" data-action="pin-card" data-id="${esc(note.id)}" data-title="${esc(note.title || '')}" aria-pressed="${note.pinned}" aria-label="${label}" title="${label}">${ICON.pin}</button>`;
}

function card(note, { words = [], showFolder = false, why = null } = {}) {
  const where = showFolder && note.directory ? `<span class="chip accent">${esc(note.directory)}</span>` : '';
  const text = snippet(note.body, words);
  const open = `class="card" role="link" tabindex="0" draggable="true" data-action="open-note" data-id="${esc(note.id)}" data-title="${esc(note.title)}" data-from="${esc(note.directory || '')}"`;
  return `<div ${open}>
    <div class="card-title"><span>${words.length ? highlight(note.title, words) : esc(note.title)}</span>${pinButton(note)}</div>
    ${text ? `<div class="card-snippet">${text}</div>` : ''}
    ${why ? `<div class="card-why">${ICON.map}${why.kind === 'idea' ? `Through the idea “${esc(why.name)}”` : why.kind === 'meaning' ? 'Close in meaning, though the words differ' : 'Through its summary'}</div>` : ''}
    <div class="card-meta">${where}<span title="${esc(`Edited ${rel(note.updated_at)}`)}">${rel(note.updated_at)}</span>${progress(note)}</div>
  </div>`;
}

function empty(icon, title, text, actions = '') {
  return `<div class="empty">${icon}<h3>${esc(title)}</h3><p>${esc(text)}</p>${actions}</div>`;
}

const skeletonAt = {};
const SKETCH_MS = 400;
const SKETCH_WIDTHS = [[33, 64, 47], [27, 69, 42], [37, 60, 51]];
function skeleton(n, surface = 'page') {
  const now = performance.now();
  const draw = now - (skeletonAt[surface] ?? -Infinity) >= SKETCH_MS && !document.querySelector(`.skeleton[data-sketch="${surface}"]`);
  skeletonAt[surface] = now;
  const line = (width, seed) => `<svg viewBox="0 0 400 6" preserveAspectRatio="none" style="width: ${width}%" aria-hidden="true"><path d="${marginWobble(1, 3, 398, false, seed, 0.6)}"/></svg>`;
  return Array.from({ length: n }, (_, i) => `<div class="skeleton${draw ? ' sk-draw' : ''}" data-sketch="${surface}">${SKETCH_WIDTHS[i % 3].map((width, j) => line(width, i * 3 + j + 1)).join('')}</div>`).join('');
}

let state = { view: null };
let seq = 0;

const SEARCH_HIGHLIGHT_MS = 5000;
const SEARCH_HIGHLIGHT_MOST = 300;
const SEARCH_HIGHLIGHT_NODES = 20000;
let searchHighlightTimer;
let searchPassage = null;
let openingNote = null;
let navigationRequest = 0;

function clearSearchHighlight() {
  clearTimeout(searchHighlightTimer);
  CSS.highlights?.delete('leo-search');
}

function highlightSearch(root, query) {
  clearSearchHighlight();
  if (!CSS.highlights || typeof Highlight === 'undefined') return false;
  const words = Array.from(String(query || '')).slice(0, 2000).join('').split(/\s+/).map(word => word.replace(/^#/, '')).filter(Boolean).slice(0, 20);
  if (!words.length) return false;
  const pattern = new RegExp(words.map(word => word.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|'), 'gi');
  const walk = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const ranges = [];
  let node;
  let seen = 0;
  while (ranges.length < SEARCH_HIGHLIGHT_MOST && seen++ < SEARCH_HIGHLIGHT_NODES && (node = walk.nextNode())) {
    if (node.parentElement.closest('textarea, script, style, .katex-mathml, .note-facts')) continue;
    pattern.lastIndex = 0;
    let match;
    while (ranges.length < SEARCH_HIGHLIGHT_MOST && (match = pattern.exec(node.textContent))) {
      const range = document.createRange();
      range.setStart(node, match.index);
      range.setEnd(node, match.index + match[0].length);
      ranges.push(range);
    }
  }
  if (!ranges.length) return false;
  CSS.highlights.set('leo-search', new Highlight(...ranges));
  const first = ranges[0];
  const rect = first.getBoundingClientRect();
  if (rect.top < 80 || rect.bottom > window.innerHeight) window.scrollBy({ top: (rect.top + rect.bottom - window.innerHeight) / 2, behavior: 'instant' });
  searchHighlightTimer = setTimeout(clearSearchHighlight, SEARCH_HIGHLIGHT_MS);
  return true;
}

window.addEventListener('hashchange', () => {
  clearSearchHighlight();
  if (searchPassage && searchPassage.hash !== location.hash) searchPassage = null;
});

function showSaveFailure(s, error, retry, current) {
  history.replaceState(history.state, '', s.note.id ? noteHash(s.note.id) : s.note.directory ? `#/new/${enc(s.note.directory)}` : '#/new');
  if (!s.persisted || error instanceof Locked) return fail(error);
  toast(error instanceof Offline ? "Can't reach leo. Your draft is kept in this browser." : error.message || 'leo could not save this note. Your draft is kept in this browser.', {
    bad: true,
    action: 'Leave, draft kept',
    run: () => { if (current() && s.persisted) return retry(s); },
  });
}

async function go(hash, { replace = false, search = null, note = null, isCurrent = null, kept = null } = {}) {
  const request = ++navigationRequest;
  const leaving = state.session;
  const mine = seq;
  const current = () => request === navigationRequest && mine === seq && state.session === leaving && (!isCurrent || isCurrent());
  if (leaving) {
    leaving.doc.stop();
    if (!(kept === leaving && leaving.persisted)) {
      try { await flush(leaving); }
      catch (error) {
        if (current()) showSaveFailure(leaving, error, saved => go(hash, { replace, search, note, isCurrent, kept: saved }), current);
        return;
      }
    }
    if (!current()) return;
  }
  if (!current()) return;
  if (leaving) {
    leaving.doc.destroy();
    state.session = null;
  }
  openingNote = note;
  clearSearchHighlight();
  const query = search ?? (state.view === 'search' ? state.query : null);
  searchPassage = query && hash.startsWith('#/n/') ? { hash, query } : null;
  if (location.hash === hash) {
    marginLeave();
    return route();
  }
  if (replace) {
    history.replaceState(history.state, '', hash);
    marginLeave();
    route();
  } else {
    location.hash = hash;
  }
}

const folderHash = (dir) => (dir ? `#/f/${enc(dir)}` : '#/');
const noteHash = (id) => `#/n/${enc(id)}`;

const asideHash = (hash) => /^#\/(settings|trash)(\/|$)/.test(hash || '');
const entryAt = () => (Number.isInteger(history.state?.leoAt) ? history.state.leoAt : null);
const places = { before: '', from: '', now: location.hash, at: entryAt() ?? 0, fromAt: -1 };
addEventListener('hashchange', (e) => {
  try {
    places.now = new URL(e.oldURL).hash;
  } catch (err) {}
  if (asideHash(location.hash) && !asideHash(places.now)) places.before = places.now || '#/';
  places.from = places.now;
  places.now = location.hash;
  let at = entryAt();
  if (at === null) {
    at = places.at + 1;
    try { history.replaceState({ ...(history.state || {}), leoAt: at }, '', location.hash); } catch (err) {}
  }
  places.fromAt = places.at;
  places.at = at;
});

function goBack(hash) {
  if (places.from === hash && places.fromAt === places.at - 1 && location.hash !== hash) return history.back();
  go(hash);
}

function back() {
  if (state.view === 'keeping' && /^#\/settings\/keeping(?:\/|$)/.test(location.hash)) return goBack(/^#\/settings(?:\/[^/]+)?\/?$/.test(places.from) ? places.from : places.before || '#/settings');
  if (asideHash(location.hash) && places.before) return goBack(places.before);
  if (state.view === 'note') return goBack('#/');
  goBack('#/');
}

function chrome({ dir = '' } = {}) {
  floating.innerHTML = '';
  document.title = dir ? `${folderLabel(dir)} · leo` : 'leo';
}

async function showWorkspace(dir = null) {
  const mine = ++seq;
  if (dir !== null) {
    const folders = await api('/api/folders');
    if (mine !== seq) return;
    side.folders = folders;
    side.loaded = true;
    if (dir && !folders.some((folder) => folder.name === dir)) toast('That folder is not there any more. Choose another folder.', { bad: true });
    else revealFolder(dir);
    history.replaceState(history.state, '', '#/');
  }
  if (state.session?.doc) state.session.doc.destroy();
  state = { view: 'workspace', dir: '' };
  document.body.classList.remove('on-note');
  chrome();
  swap('workspace', () => {
    app.innerHTML = `<section class="workspace-empty"><header class="home-head"><h1 class="home-day" id="home-day"></h1><p class="home-line" id="home-line"></p><div class="home-week" id="home-week" role="group" aria-label="This week"></div></header><hr class="home-rule"><label class="quick-bar home-ask">${ICON.search}<input id="home-ask" placeholder="Search notes, or ask Felix" autocomplete="off" spellcheck="false" enterkeyhint="go" maxlength="2000" aria-label="Search notes, or ask Felix"><kbd>${shortcutText(shortcutKeys('quick'))}</kbd></label><p class="hint home-dictate" hidden></p><section class="home-today" id="home-today" aria-label="Today" tabindex="-1" hidden></section><section class="home-recent${(homeRecent.notes ? homeRecent.notes.length : side.notes.get('')?.items?.length) ? ' reserve' : ''}" id="home-recent" aria-label="Recently written"></section><div class="home-foot">${DRAWN.arrow}<p id="home-foot"></p>${!sideWide() ? '<button class="btn plain" data-action="folders">Folders</button>' : ''}<button class="btn plain" data-action="new" title="New note (${shortcutText(shortcutKeys('new'))})">${ICON.filePlus}New note</button></div></section>`;
    wireHomeAsk();
    drawTasksWindow();
    drawHomeToday();
    loadHomeWeek();
    if (!(Date.now() - tasksWin.eventsAt < CAL_POLL_MS)) loadTasksWindow().catch(() => {});
    dictateHint($('.home-dictate'));
    scrollToTop();
  });
}

const SHORTCUT_KEYS = { option: ['⌥', 'Alt'], opt: ['⌥', 'Alt'], alt: ['⌥', 'Alt'], ctrl: ['⌃', 'Ctrl'], control: ['⌃', 'Ctrl'], cmd: ['⌘', 'Win'], command: ['⌘', 'Win'], meta: ['⌘', 'Win'], super: ['⌘', 'Win'], shift: ['⇧', 'Shift'] };
function shortcutParts(keys) {
  return keys ? keys.split('+').map((key) => { const name = key.trim().replace(/^(ctrl|option|alt|shift|command|super)_(left|right)$/i, '$1'); return SHORTCUT_KEYS[name.toLowerCase()]?.[ON_MAC ? 0 : 1] || name.replace(/^./, (c) => c.toUpperCase()); }) : [];
}
function shortcutText(keys) {
  return shortcutParts(keys).join(ON_MAC ? '' : '+');
}
const SHORTCUTS = { quick: ['Quick Leo', 'k'], new: ['New note', 'n'], ask: ['Ask Felix about a selection', 'shift+e'] };
function shortcutDefault(name) {
  return `${ON_MAC ? 'cmd' : 'ctrl'}+${SHORTCUTS[name][1]}`;
}
function shortcutKeys(name) {
  try {
    const saved = JSON.parse(localStorage.getItem('leo-shortcuts'))?.[name];
    if (typeof saved === 'string' && /(^|\+)(cmd|ctrl)\+./.test(saved)) return saved;
  } catch (e) {}
  return shortcutDefault(name);
}
function chordOf(e, modifiers) {
  let key = String(e.key || '').toLowerCase();
  const code = /^(?:Key|Digit)(.)$/.exec(e.code || '')?.[1].toLowerCase();
  if (code && ((ON_MAC && e.altKey) || (e.shiftKey && /\d/.test(code)))) key = code;
  const held = [e.metaKey && 'cmd', e.ctrlKey && 'ctrl', e.altKey && 'alt', e.shiftKey && 'shift'].filter(Boolean);
  if (!key || /^(meta|control|alt|shift)$/.test(key)) return modifiers ? held.join('+') : '';
  return held.concat(key === ' ' ? 'space' : key).join('+');
}
function pressed(e, name) {
  return chordOf(e) === shortcutKeys(name);
}
function shortcutAria(keys) {
  return keys.split('+').map((key) => ({ cmd: 'Meta', ctrl: 'Control', alt: 'Alt', shift: 'Shift' })[key] || key.toUpperCase()).join('+');
}
let dictateKeys = null;
function dictateKeysSeen(view) {
  dictateKeys = view?.settings?.shortcut || null;
}
async function dictateHint(el) {
  try {
    if (dictateKeys === null) dictateKeysSeen(await api('/api/dictation'));
    const keys = dictateKeys;
    if (keys && el?.isConnected) { el.textContent = `Press ${shortcutText(keys)} anywhere to dictate.`; el.hidden = false; }
  } catch (e) {}
}

function wireHomeAsk() {
  const field = $('#home-ask');
  const open = () => {
    quickLeo();
    const input = $('#quick-input');
    input.value = field.value;
    if (input.value) input.dispatchEvent(new Event('input'));
  };
  field.addEventListener('click', open);
  field.addEventListener('input', open);
  field.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter' || e.shiftKey || e.isComposing) return;
    e.preventDefault();
    open();
  });
}

let moreWatch = null;

async function undoTrashMove(done, from) {
  try {
    const back = await api('/api/trash/restore', { method: 'POST', body: { ids: done.ids, dirs: done.dirs || [] } });
    toast(back.restored === done.ids.length ? `Brought back “${folderLabel(from)}”` : `Brought back ${plural(back.restored, 'note')}`);
    await refreshSideTree();
  } catch (e) { fail(e); }
}

const cleanTitle = (text) => text.replace(/\s*\n\s*/g, ' ').trim();

const SAVE_INK = { 'Editing…': 'wet', 'Saving…': 'wet', Saved: 'dry' };
function mark(s, text) {
  if (state.session !== s) return;
  const el = $('#save-state');
  if (!el) return;
  const ink = SAVE_INK[text];
  el.className = ink ? `save-${ink}` : '';
  el.innerHTML = ink ? `<svg class="save-drop" aria-hidden="true"><use href="#leo-mark"/></svg><span class="said">${esc(text)}</span>` : esc(text);
  const edited = $('#note-edited');
  if (!edited) return;
  edited.hidden = /^Not saved/.test(text);
  if (text === 'Saved') edited.textContent = `Edited ${rel(new Date().toISOString())}`;
}

let local;
try { local = window.localStorage; } catch (_) { local = null; }
const saving = window.leoSaving.create({
  api, storage: local,
  mark,
  storageError: () => toast('This browser cannot keep a draft. Keep the page open until your note says Saved.', { bad: true }),
  recovered: (count) => {
    toast(`Saving ${count} edit${count === 1 ? '' : 's'} that had not reached leo yet.`);
    setTimeout(() => saving.retry().then(() => {
      const left = saving.drafts().length;
      if (left) toast(`${plural(left, 'edit')} still could not be saved. Try again or discard ${left === 1 ? 'it' : 'them'} in Settings → Keeping.`, { bad: true });
    }), 0);
  },
  created: (s, note) => {
    if (state.session === s) history.replaceState(history.state, '', noteHash(note.id));
    marginCreated(s, note).catch(fail);
  },
  conflict: (s, note) => {
    if (state.session === s) {
      history.replaceState(history.state, '', noteHash(note.id));
      s.title.textContent = s.edit.title;
    }
    toast('This note changed elsewhere. Your edits were kept in a separate copy.', { action: 'Open copy', run: () => go(noteHash(note.id)) });
  },
  retryable: (error) => error instanceof Offline || error.status >= 500,
});

function snapshot(s, text = () => s.doc.source()) {
  const typed = cleanTitle(s.title.textContent);
  const { id, title } = s.note;
  return {
    get title() { return typed || (id ? title : md.plain(text()).slice(0, 60)); },
    get body() { return text(); },
  };
}

function changed(s, text) { saving.changed(s, snapshot(s, text)); }
function flush(s = state.session) { return saving.flush(s); }

let citedLines = null;

function showCitedLines() {
  const want = citedLines;
  citedLines = null;
  if (!want || !state.session || !state.session.doc) return;
  if (!state.session.doc.highlight(want.from, want.to)) toast('Those lines have changed since Felix read the note.');
}

async function showPassage(note, name, at) {
  const mine = seq;
  const found = await api(`/api/notes/${enc(note)}/originals/${enc(name)}/passage?at=${enc(at || 'p1')}`);
  if (mine !== seq) return;
  const lines = found.lines.map((l) => `<div class="source-row"><span class="rec-at">${l.n}</span><span class="passage-text">${esc(l.text)}</span></div>`).join('');
  const box = sheet(`<h3>${esc(name)}</h3>
    <p>${esc(found.label[0].toUpperCase() + found.label.slice(1))}, word for word from the file you uploaded.</p>
    <div class="source-list passage-list">${lines || '<p class="hint">That part of the file is empty.</p>'}</div>
    ${sheetButtons(viewKind(name) ? ['Open the file', 'id="passage-open"'] : null, { cancel: 'Close' })}`);
  const open = $('#passage-open', box);
  if (open) open.addEventListener('click', () => { closeSheet(true); viewOriginal(note, name, found.page).catch(fail); });
}

function readLine(text) {
  const time = md.readTime(text);
  return time ? ` · ${time}` : '';
}

let readTimer = 0;
let readWanted = 0;
function readSoon(text) {
  const line = $('#note-read');
  const mine = ++readWanted;
  clearTimeout(readTimer);
  readTimer = setTimeout(() => marginIdle(() => {
    if (line && mine === readWanted) line.textContent = readLine(text());
  }), 300);
}

async function showNote(id, { fresh = null, draft = null } = {}) {
  const passage = searchPassage?.hash === noteHash(id) ? searchPassage : null;
  searchPassage = null;
  clearSearchHighlight();
  const mine = ++seq;
  const note = fresh || (await api(`/api/notes/${enc(id)}`));
  await marginLoadPrefs();
  await saving.ready;
  if (mine !== seq) return;
  let s;
  const opened = swap(`n:${note.id || 'new'}`, () => {
    scrollToTop();
    document.body.classList.toggle('on-note', true);
    if (source.note && source.note !== note.id) closeSource();
    if (state.session && state.session.doc) state.session.doc.destroy();
    s = draft || saving.open(note);
    if (side.openedNote !== note.id) side.target = null;
    side.openedNote = note.id;
    state = { view: 'note', dir: note.directory, session: s };
    chrome({ dir: note.directory });
    document.title = `${note.title || 'New note'} · leo`;
    chat.setContext(note.id ? { id: note.id, title: note.title, directory: note.directory } : null);
    const when = note.id ? `Edited ${rel(note.updated_at)}` : 'New note';
    app.innerHTML = `<div class="note-layout"><div class="note-columns"><aside class="note-outline" aria-label="Note outline" hidden><details class="source-more"><summary aria-label="Headings in this note" title="Headings in this note"><span class="outline-icon">${ICON.list}</span>${ICON.chevron}</summary><nav aria-label="Headings"></nav></details></aside><article class="note"><div class="page-marks"><span class="page-mark" data-edge="left" role="separator" aria-orientation="vertical" tabindex="0" aria-label="Note width: drag to change, double-click to reset"></span><span class="page-mark" data-edge="right" role="separator" aria-orientation="vertical" tabindex="0" aria-label="Note width: drag to change, double-click to reset"></span><span class="page-width-label" hidden></span></div>
    <header class="note-head"><div class="note-meta" data-tauri-drag-region><div class="margin-mode-wrap"><button type="button" id="margin-persona" class="margin-mode" aria-haspopup="menu" aria-expanded="false" aria-controls="margin-persona-menu" disabled><span id="margin-persona-icon" class="margin-mode-icon" aria-hidden="true">${ICON.book}</span><span id="margin-persona-label">Felix as Reader</span>${ICON.chevron}</button><div id="margin-persona-menu" class="margin-mode-menu margin-persona-menu" role="menu" aria-label="Who reads this note" hidden></div></div><div class="margin-mode-wrap"><button type="button" id="margin-mode" class="margin-mode" aria-haspopup="menu" aria-expanded="false" aria-controls="margin-mode-menu" disabled><span id="margin-mode-icon" class="margin-mode-icon" aria-hidden="true">${ICON.pen}</span><span id="margin-mode-label">When you ask</span>${ICON.chevron}</button><div id="margin-mode-menu" class="margin-mode-menu" role="menu" aria-label="Note reading mode" hidden><button type="button" role="menuitemradio" aria-checked="false" data-margin-mode="along">Reads along<small>Reads changes after a pause.</small></button><button type="button" role="menuitemradio" aria-checked="true" data-margin-mode="ask">When you ask<small>Reads nothing until you ask.</small></button></div></div><button type="button" id="margin-through" class="margin-mode margin-through" aria-disabled="true" title="Felix reads the whole note and leaves margin notes. The text goes to your writing AI."><span id="margin-through-icon" class="margin-mode-icon" aria-hidden="true">${ICON.readThrough}</span><span id="margin-through-label">Read through</span></button><button type="button" id="note-more" class="margin-mode note-more" data-action="note-menu" aria-label="More for this note" aria-haspopup="menu" aria-expanded="false">${ICON.more}</button><span id="margin-status" role="status" class="hint"></span></div>
    <h1 id="title" contenteditable="plaintext-only" spellcheck="true" data-placeholder="Title" enterkeyhint="next">${esc(s.edit.title)}</h1><p class="note-facts" id="note-facts"><span id="save-state"></span><span id="note-edited">${when}</span><span id="note-read">${esc(readLine(s.edit.body))}</span></p><div class="note-originals"></div><div id="margin-instructions" class="margin-instructions" data-read-field hidden><label class="field">${ICON.pen}<input data-reads="${MARGIN_INSTRUCTIONS_READS}" placeholder="Instructions for this note, e.g. check that each claim has a source" aria-label="Instructions for this note" autocomplete="off"></label><p class="hint" data-read-hint role="status" hidden>Felix reads the first ${MARGIN_INSTRUCTIONS_READS} characters of this.</p></div>${note.writeup_pending ? '<div class="set-card writeup-pending"><h3>These notes are not written up yet</h3><p class="set-note">No writing AI was ready, so leo kept the transcript as it is.</p><div class="buttons"><a class="btn primary sm" href="#/settings/writing">Set up writing AI</a><button type="button" class="btn plain sm" data-action="note-sources">Write the notes</button></div></div>' : ''}</header>
    <div class="prose doc" id="doc"></div>
    <section class="related" id="related" hidden aria-label="Related notes"></section>
  </article></div></div>`;
    s.title = $('#title');
    drawOriginals(note);
    hideAskFloat();
    s.doc = leoDoc.mount($('#doc'), { source: s.edit.body, outline: $('.note-outline'), onChange: (text) => { changed(s, text); marginEdited(s); readSoon(text); }, placeholder: 'Write, or paste anything', dir: note.directory || '', onPictures: (files) => addPictures(s, files), onSelection: (has) => placeAskFloat(has), noteTools: true });
    marginMount(s, mine).catch(fail);
    if (citedLines && citedLines.id === note.id) showCitedLines();
    else if (passage && highlightSearch(app.querySelector('article.note'), passage.query)) {
      clearTimeout(reading.timer);
      reading.id = note.id;
      reading.user = false;
    } else restoreReading(note.id, mine);
  });
  if (opened) arriveOnPage($('#doc'));
  drawRelated(note.id, mine);
  if (s.dirty) {
    mark(s, 'Recovered draft · saving…');
    flush(s).catch(fail);
  }
  const blank = () => {
    const empty = !cleanTitle(s.title.textContent);
    if (empty && s.title.innerHTML !== '') s.title.textContent = '';
    s.title.classList.toggle('blank', empty);
  };
  blank();
  s.title.addEventListener('input', () => {
    blank();
    document.title = `${cleanTitle(s.title.textContent) || 'Untitled'} · leo`;
    changed(s);
  });
  s.title.addEventListener('keydown', (e) => {
    if (e.isComposing) return;
    if (e.key === 'Enter' || (e.key === 'Tab' && !e.shiftKey)) {
      e.preventDefault();
      s.doc.editStart();
    }
  });
  s.title.addEventListener('paste', (e) => {
    e.preventDefault();
    const text = cleanTitle((e.clipboardData || window.clipboardData).getData('text'));
    document.execCommand('insertText', false, text);
  });
  if (!note.id) {
    s.title.focus();
  }
}


function newNote(dir) {
  return showNote(null, { fresh: { id: null, title: '', body: '', directory: dir || '', pinned: false, version: null } });
}

async function ready() {
  const s = state.session;
  if (!s) return null;
  s.doc.stop();
  await flush(s);
  return s.note.id ? s : null;
}

async function showSearch(query) {
  const mine = ++seq;
  state = { view: 'search', dir: '', query };
  chrome();
  openSearch(query);
  if (!query.trim()) {
    app.innerHTML = empty(ICON.search, 'Search every note', 'Titles, text and the ideas. Abbreviations like BFS work too.');
    return;
  }
  app.innerHTML = `<div class="section-title">Searching…</div><div class="cards">${skeleton(3, 'search')}</div>`;
  let results;
  try {
    results = await api(`/api/search?q=${enc(query)}&brief=true&limit=${SEARCH_STEP + 1}&offset=0`);
  } catch (e) {
    if (mine === seq) app.innerHTML = empty(ICON.search, 'Search did not finish', e.message || 'Something went wrong.', `<button class="btn primary" data-action="search-retry" data-query="${esc(query)}">Try again</button>`);
    return;
  }
  if (mine !== seq) return;
  const words = query.split(/\s+/).map((w) => w.replace(/^#/, '')).filter(Boolean);
  const draw = (list) => list.map((n) => card(n, { words, showFolder: true, why: n.why })).join('');
  const more = results.length > SEARCH_STEP, shown = results.slice(0, SEARCH_STEP);
  app.innerHTML = shown.length
    ? `<div class="section-title" id="search-count">${searchCount(shown.length, more)}</div><div class="cards">${draw(shown)}</div>${more ? '<button type="button" class="btn plain more-notes" id="more-notes" data-action="search-more">Show more</button>' : ''}`
    : empty(DRAWN.search, `No notes match “${query}”`, 'Try fewer words, or ask Felix to look for the idea.', `<button class="btn primary" data-action="search-ask" data-query="${esc(query)}">${ICON.chat}Ask Felix</button><button class="btn plain" data-action="search-new" data-query="${esc(query)}">${ICON.filePlus}New note “${esc(query)}”</button>`);
  cascade($('.cards'));
  if (moreWatch) moreWatch.disconnect();
  searchPaging = { mine, query, loaded: shown.length, loading: false, draw };
  const button = $('#more-notes');
  if (!button || typeof IntersectionObserver !== 'function') return;
  moreWatch = new IntersectionObserver((seen) => {
    if (seen.some((x) => x.isIntersecting)) showMore();
  }, { rootMargin: '600px 0px' });
  moreWatch.observe(button);
}

const SEARCH_STEP = 60;
let searchPaging = null;
function searchCount(loaded, more) {
  return plural(loaded, 'note') + (more ? ' so far' : '');
}
async function showMore() {
  const page = searchPaging, button = $('#more-notes');
  if (!page || page.mine !== seq || page.loading || !button) return;
  page.loading = true;
  button.disabled = true;
  button.textContent = 'Loading…';
  let answer;
  try {
    answer = await api(`/api/search?q=${enc(page.query)}&brief=true&limit=${SEARCH_STEP + 1}&offset=${page.loaded}`);
  } catch (e) {
    if (page.mine !== seq) return;
    page.loading = false;
    button.disabled = false;
    button.textContent = 'Try again';
    return;
  }
  if (page.mine !== seq) return;
  page.loading = false;
  const more = answer.length > SEARCH_STEP, shown = answer.slice(0, SEARCH_STEP);
  $('.cards').insertAdjacentHTML('beforeend', page.draw(shown));
  page.loaded += shown.length;
  $('#search-count').textContent = searchCount(page.loaded, more);
  if (more) {
    button.disabled = false;
    button.textContent = 'Show more';
  } else {
    if (moreWatch) moreWatch.disconnect();
    button.remove();
  }
}

async function showRecord(arg) {
  const [dir] = arg.split('?');
  const asked = new URLSearchParams(location.hash.split('?')[1] || '');
  const mine = ++seq;
  state = { view: 'record', dir: '' };
  chrome();
  document.title = 'Record · leo';
  app.innerHTML = '<div id="rec-box"></div>';
  let folders = [];
  try {
    folders = await api('/api/folders');
  } catch (e) {
    if (e instanceof Locked) throw e;
  }
  if (mine !== seq) return;
  await recorder.show($('#rec-box'), { dir, folders: folders.map((f) => f.name).filter(Boolean), event: asked.get('event'), kind: asked.get('kind') });
}

window.addEventListener('beforeprint', () => {
  for (const el of document.body.querySelectorAll('*')) {
    if (getComputedStyle(el).position === 'fixed') el.setAttribute('data-print-off', '');
  }
});
window.addEventListener('afterprint', () => {
  document.body.querySelectorAll('[data-print-off]').forEach((el) => el.removeAttribute('data-print-off'));
});
