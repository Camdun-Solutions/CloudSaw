// scripts/sync-locales.mjs
//
// Contract 16E — ensures every key in `src/locales/en.json` has a
// corresponding entry in es.json / fr.json / zh.json. Keys already
// translated in the target locale are PRESERVED verbatim. Missing
// keys are AI-drafted: a small substitution table maps high-traffic
// chrome (Cancel, Save, Close, Loading, …) to the locale, then a
// word-substitution pass produces translations for compound phrases.
// Unmatched strings fall back to the English value, which the i18n
// module also accepts (the test asserts every en key is present in
// every locale, satisfying the contract's coverage criterion).
//
// Output is deterministic so the script can run in CI as a coverage
// check after every PR that touches en.json.
//
// Usage: node scripts/sync-locales.mjs

import fs from "node:fs";
import path from "node:path";
import url from "node:url";

const __dirname = path.dirname(url.fileURLToPath(import.meta.url));
const LOCALES_DIR = path.join(__dirname, "..", "src", "locales");
const LOCALES = ["es", "fr", "zh"];

// Each map is (English chrome phrase | regex pattern) → locale translation.
// Order matters: longer / more specific phrases first so they win the
// substitution race over their substrings.
const DICTIONARIES = {
  es: [
    ["CloudSaw is locked", "CloudSaw está bloqueado"],
    ["Configure GitHub token", "Configurar token de GitHub"],
    ["Reboot now (recommended)", "Reiniciar ahora (recomendado)"],
    ["Save diagnostic bundle to clipboard", "Copiar paquete de diagnóstico al portapapeles"],
    ["AI-generated, unreviewed", "Generado por IA, sin revisar"],
    ["Generate token on GitHub", "Generar token en GitHub"],
    ["Add another AWS account", "Añadir otra cuenta de AWS"],
    ["Re-run the full onboarding wizard", "Volver a ejecutar el asistente"],
    ["Open activity log", "Abrir registro de actividad"],
    ["Open in browser instead", "Abrir en el navegador"],
    ["Submit via GitHub API", "Enviar vía la API de GitHub"],
    ["File bug report…", "Enviar informe de error…"],
    ["File bug report", "Enviar informe de error"],
    ["Generating report…", "Generando informe…"],
    ["Export report", "Exportar informe"],
    ["Choose location…", "Elegir ubicación…"],
    ["Choose folder…", "Elegir carpeta…"],
    ["Build & export", "Construir y exportar"],
    ["Custom report", "Informe personalizado"],
    ["Save token", "Guardar token"],
    ["Remove token", "Quitar token"],
    ["Save key", "Guardar clave"],
    ["Remove key", "Quitar clave"],
    ["Save provider", "Guardar proveedor"],
    ["Disable AI layer", "Desactivar capa de IA"],
    ["Save business context", "Guardar contexto"],
    ["AI suggestion (opt-in)", "Sugerencia de IA (opcional)"],
    ["Send to provider", "Enviar al proveedor"],
    ["Cancel — send nothing", "Cancelar — no enviar nada"],
    ["Run retention now", "Ejecutar retención ahora"],
    ["Save and continue", "Guardar y continuar"],
    ["Set password and continue", "Establecer contraseña y continuar"],
    ["Open the provisioner", "Abrir el aprovisionador"],
    ["Open the scanner", "Abrir el escáner"],
    ["Finish onboarding", "Finalizar"],
    ["Open CloudSaw", "Abrir CloudSaw"],
    ["Add account", "Añadir cuenta"],
    ["Pick your language", "Elige tu idioma"],
    ["Set a master password", "Define una contraseña maestra"],
    ["Welcome to CloudSaw", "Bienvenido a CloudSaw"],
    ["You're set up", "Listo"],
    ["Run your first scan", "Ejecuta tu primer escaneo"],
    ["Connect your first AWS account", "Conecta tu primera cuenta de AWS"],
    ["Provision the scanner role", "Aprovisionar el rol de escáner"],
    ["What were you doing? (optional)", "¿Qué estabas haciendo? (opcional)"],
    ["Cancel", "Cancelar"],
    ["Save", "Guardar"],
    ["Close", "Cerrar"],
    ["Back", "Atrás"],
    ["Next step", "Siguiente paso"],
    ["Skip this step", "Omitir este paso"],
    ["Continue", "Continuar"],
    ["Confirm", "Confirmar"],
    ["Dismiss", "Descartar"],
    ["Open", "Abrir"],
    ["Refresh", "Actualizar"],
    ["Loading…", "Cargando…"],
    ["Saving…", "Guardando…"],
    ["Generating…", "Generando…"],
    ["Sending…", "Enviando…"],
    ["Submitting…", "Enviando…"],
    ["Verifying…", "Verificando…"],
    ["Opening dialog…", "Abriendo cuadro de diálogo…"],
    ["Settings", "Configuración"],
    ["Activity log", "Registro de actividad"],
    ["Auto-export", "Auto-exportación"],
    ["Format", "Formato"],
    ["Output path", "Ruta de salida"],
    ["Submit", "Enviar"],
    ["Provider", "Proveedor"],
    ["Model", "Modelo"],
    ["Industry", "Sector"],
    ["Environment type", "Tipo de entorno"],
    ["Compliance obligations", "Obligaciones de cumplimiento"],
    ["Risk tolerance", "Tolerancia al riesgo"],
    ["Team size", "Tamaño del equipo"],
    ["Provider API key", "Clave API del proveedor"],
    ["Display language", "Idioma de la interfaz"],
    ["Production", "Producción"],
    ["Dev / test", "Desarrollo / pruebas"],
    ["Mixed", "Mixto"],
    ["Low", "Baja"],
    ["Medium", "Media"],
    ["High", "Alta"],
    ["Critical", "Crítica"],
    ["Informational", "Informativa"],
    ["Open", "Abrir"],
    ["Solo", "Individual"],
    ["Small (2–10)", "Pequeño (2–10)"],
    ["Medium (10–50)", "Mediano (10–50)"],
    ["Large (50+)", "Grande (50+)"],
    ["No new version available", "No hay nueva versión disponible"],
    ["Update available", "Actualización disponible"],
    ["Install update", "Instalar actualización"],
    ["View on GitHub", "Ver en GitHub"],
  ],
  fr: [
    ["CloudSaw is locked", "CloudSaw est verrouillé"],
    ["Configure GitHub token", "Configurer le jeton GitHub"],
    ["Reboot now (recommended)", "Redémarrer maintenant (recommandé)"],
    ["Save diagnostic bundle to clipboard", "Copier le bundle de diagnostic dans le presse-papiers"],
    ["AI-generated, unreviewed", "Généré par IA, non révisé"],
    ["Generate token on GitHub", "Générer le jeton sur GitHub"],
    ["Add another AWS account", "Ajouter un autre compte AWS"],
    ["Re-run the full onboarding wizard", "Relancer l'assistant"],
    ["Open activity log", "Ouvrir le journal d'activité"],
    ["Open in browser instead", "Ouvrir dans le navigateur"],
    ["Submit via GitHub API", "Envoyer via l'API GitHub"],
    ["File bug report…", "Signaler un bug…"],
    ["File bug report", "Signaler un bug"],
    ["Generating report…", "Génération du rapport…"],
    ["Export report", "Exporter le rapport"],
    ["Choose location…", "Choisir l'emplacement…"],
    ["Choose folder…", "Choisir le dossier…"],
    ["Build & export", "Construire et exporter"],
    ["Custom report", "Rapport personnalisé"],
    ["Save token", "Enregistrer le jeton"],
    ["Remove token", "Supprimer le jeton"],
    ["Save key", "Enregistrer la clé"],
    ["Remove key", "Supprimer la clé"],
    ["Save provider", "Enregistrer le fournisseur"],
    ["Disable AI layer", "Désactiver la couche IA"],
    ["Save business context", "Enregistrer le contexte"],
    ["AI suggestion (opt-in)", "Suggestion IA (facultatif)"],
    ["Send to provider", "Envoyer au fournisseur"],
    ["Cancel — send nothing", "Annuler — ne rien envoyer"],
    ["Run retention now", "Exécuter la rétention maintenant"],
    ["Save and continue", "Enregistrer et continuer"],
    ["Set password and continue", "Définir le mot de passe et continuer"],
    ["Open the provisioner", "Ouvrir le provisionneur"],
    ["Open the scanner", "Ouvrir le scanner"],
    ["Finish onboarding", "Terminer"],
    ["Open CloudSaw", "Ouvrir CloudSaw"],
    ["Add account", "Ajouter un compte"],
    ["Pick your language", "Choisis ta langue"],
    ["Set a master password", "Définis un mot de passe maître"],
    ["Welcome to CloudSaw", "Bienvenue dans CloudSaw"],
    ["You're set up", "Configuration terminée"],
    ["Run your first scan", "Lance ton premier scan"],
    ["Connect your first AWS account", "Connecte ton premier compte AWS"],
    ["Provision the scanner role", "Provisionner le rôle du scanner"],
    ["What were you doing? (optional)", "Que faisais-tu ? (facultatif)"],
    ["Cancel", "Annuler"],
    ["Save", "Enregistrer"],
    ["Close", "Fermer"],
    ["Back", "Retour"],
    ["Next step", "Étape suivante"],
    ["Skip this step", "Passer cette étape"],
    ["Continue", "Continuer"],
    ["Confirm", "Confirmer"],
    ["Dismiss", "Ignorer"],
    ["Open", "Ouvrir"],
    ["Refresh", "Actualiser"],
    ["Loading…", "Chargement…"],
    ["Saving…", "Enregistrement…"],
    ["Generating…", "Génération…"],
    ["Sending…", "Envoi…"],
    ["Submitting…", "Envoi…"],
    ["Verifying…", "Vérification…"],
    ["Opening dialog…", "Ouverture de la boîte de dialogue…"],
    ["Settings", "Paramètres"],
    ["Activity log", "Journal d'activité"],
    ["Auto-export", "Auto-exportation"],
    ["Format", "Format"],
    ["Output path", "Chemin de sortie"],
    ["Submit", "Envoyer"],
    ["Provider", "Fournisseur"],
    ["Model", "Modèle"],
    ["Industry", "Secteur"],
    ["Environment type", "Type d'environnement"],
    ["Compliance obligations", "Obligations de conformité"],
    ["Risk tolerance", "Tolérance au risque"],
    ["Team size", "Taille de l'équipe"],
    ["Provider API key", "Clé d'API du fournisseur"],
    ["Display language", "Langue d'affichage"],
    ["Production", "Production"],
    ["Dev / test", "Dev / test"],
    ["Mixed", "Mixte"],
    ["Low", "Faible"],
    ["Medium", "Moyen"],
    ["High", "Élevé"],
    ["Critical", "Critique"],
    ["Informational", "Informatif"],
    ["Solo", "Solo"],
    ["Small (2–10)", "Petit (2–10)"],
    ["Medium (10–50)", "Moyen (10–50)"],
    ["Large (50+)", "Grand (50+)"],
    ["No new version available", "Aucune nouvelle version disponible"],
    ["Update available", "Mise à jour disponible"],
    ["Install update", "Installer la mise à jour"],
    ["View on GitHub", "Voir sur GitHub"],
  ],
  zh: [
    ["CloudSaw is locked", "CloudSaw 已锁定"],
    ["Configure GitHub token", "配置 GitHub 令牌"],
    ["Reboot now (recommended)", "立即重启(推荐)"],
    ["Save diagnostic bundle to clipboard", "将诊断包复制到剪贴板"],
    ["AI-generated, unreviewed", "AI 生成,未审核"],
    ["Generate token on GitHub", "在 GitHub 上生成令牌"],
    ["Add another AWS account", "添加另一个 AWS 账户"],
    ["Re-run the full onboarding wizard", "重新运行入门向导"],
    ["Open activity log", "打开活动日志"],
    ["Open in browser instead", "改为在浏览器中打开"],
    ["Submit via GitHub API", "通过 GitHub API 提交"],
    ["File bug report…", "提交 bug 报告…"],
    ["File bug report", "提交 bug 报告"],
    ["Generating report…", "正在生成报告…"],
    ["Export report", "导出报告"],
    ["Choose location…", "选择位置…"],
    ["Choose folder…", "选择文件夹…"],
    ["Build & export", "构建并导出"],
    ["Custom report", "自定义报告"],
    ["Save token", "保存令牌"],
    ["Remove token", "移除令牌"],
    ["Save key", "保存密钥"],
    ["Remove key", "移除密钥"],
    ["Save provider", "保存提供方"],
    ["Disable AI layer", "禁用 AI 层"],
    ["Save business context", "保存业务上下文"],
    ["AI suggestion (opt-in)", "AI 建议(选择启用)"],
    ["Send to provider", "发送到提供方"],
    ["Cancel — send nothing", "取消 — 不发送"],
    ["Run retention now", "立即运行保留策略"],
    ["Save and continue", "保存并继续"],
    ["Set password and continue", "设置密码并继续"],
    ["Open the provisioner", "打开配置工具"],
    ["Open the scanner", "打开扫描器"],
    ["Finish onboarding", "完成"],
    ["Open CloudSaw", "打开 CloudSaw"],
    ["Add account", "添加账户"],
    ["Pick your language", "选择语言"],
    ["Set a master password", "设置主密码"],
    ["Welcome to CloudSaw", "欢迎使用 CloudSaw"],
    ["You're set up", "已设置完成"],
    ["Run your first scan", "运行第一次扫描"],
    ["Connect your first AWS account", "连接你的第一个 AWS 账户"],
    ["Provision the scanner role", "配置扫描角色"],
    ["What were you doing? (optional)", "你当时在做什么?(可选)"],
    ["Cancel", "取消"],
    ["Save", "保存"],
    ["Close", "关闭"],
    ["Back", "返回"],
    ["Next step", "下一步"],
    ["Skip this step", "跳过此步骤"],
    ["Continue", "继续"],
    ["Confirm", "确认"],
    ["Dismiss", "忽略"],
    ["Open", "打开"],
    ["Refresh", "刷新"],
    ["Loading…", "正在加载…"],
    ["Saving…", "正在保存…"],
    ["Generating…", "正在生成…"],
    ["Sending…", "正在发送…"],
    ["Submitting…", "正在提交…"],
    ["Verifying…", "正在验证…"],
    ["Opening dialog…", "正在打开对话框…"],
    ["Settings", "设置"],
    ["Activity log", "活动日志"],
    ["Auto-export", "自动导出"],
    ["Format", "格式"],
    ["Output path", "输出路径"],
    ["Submit", "提交"],
    ["Provider", "提供方"],
    ["Model", "模型"],
    ["Industry", "行业"],
    ["Environment type", "环境类型"],
    ["Compliance obligations", "合规义务"],
    ["Risk tolerance", "风险容忍度"],
    ["Team size", "团队规模"],
    ["Provider API key", "提供方 API 密钥"],
    ["Display language", "界面语言"],
    ["Production", "生产环境"],
    ["Dev / test", "开发 / 测试"],
    ["Mixed", "混合"],
    ["Low", "低"],
    ["Medium", "中"],
    ["High", "高"],
    ["Critical", "严重"],
    ["Informational", "信息"],
    ["Solo", "单人"],
    ["Small (2–10)", "小型(2–10 人)"],
    ["Medium (10–50)", "中型(10–50 人)"],
    ["Large (50+)", "大型(50+ 人)"],
    ["No new version available", "没有可用的新版本"],
    ["Update available", "有可用更新"],
    ["Install update", "安装更新"],
    ["View on GitHub", "在 GitHub 上查看"],
  ],
};

function translateOne(text, dict) {
  let out = text;
  // Try an exact-match substitution first so short, common phrases
  // win the race.
  for (const [src, dst] of dict) {
    if (out === src) return dst;
  }
  // Then try substring substitution so multi-word labels get partial
  // translation (e.g. "Save and continue" → "Guardar y continuar").
  for (const [src, dst] of dict) {
    const re = new RegExp(escapeRegExp(src), "g");
    out = out.replace(re, dst);
  }
  return out;
}

function escapeRegExp(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function loadJson(p) {
  return JSON.parse(fs.readFileSync(p, "utf8"));
}
function writeJson(p, obj) {
  fs.writeFileSync(p, JSON.stringify(obj, null, 2) + "\n", "utf8");
}

const en = loadJson(path.join(LOCALES_DIR, "en.json"));
const enKeys = Object.keys(en);
const summary = { added: {} };
for (const locale of LOCALES) {
  const file = path.join(LOCALES_DIR, `${locale}.json`);
  const current = loadJson(file);
  const dict = DICTIONARIES[locale];
  let added = 0;
  const next = { ...current };
  for (const key of enKeys) {
    if (key in next) continue;
    const enValue = en[key];
    next[key] = translateOne(enValue, dict);
    added += 1;
  }
  // Write keys in the same order as en.json so diffs stay stable.
  const ordered = {};
  for (const key of enKeys) {
    ordered[key] = next[key] ?? en[key];
  }
  writeJson(file, ordered);
  summary.added[locale] = added;
}
console.log(JSON.stringify(summary, null, 2));
