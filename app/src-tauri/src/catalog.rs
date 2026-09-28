use crate::{
    engine_profile,
    store::{Failure, Result},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extension {
    pub id: &'static str,
    pub name: &'static str,
    pub engine: &'static str,
    pub version: &'static str,
    pub kind: &'static str,
    pub description: &'static str,
    pub source: &'static str,
    pub web_support: &'static str,
    pub scripts: &'static str,
}

pub fn extensions() -> Vec<Extension> {
    let mut entries: Vec<Extension> = [("dsh-local-clock", "DSH"), ("pi-local-clock", "Pi")]
        .into_iter()
        .map(|(id, engine)| Extension {
            id,
            engine,
            name: "本地时间",
            version: "1.1.0",
            kind: "tool",
            description: "为模型提供当前时间与时区；不读取项目文件，不访问网络。",
            source: "栖点内置源码",
            web_support: "工具文本结果；不添加独立 Web 面板",
            scripts: "无安装脚本；仅启用后在工作台进程内执行工具代码",
        })
        .collect();
    for (id, engine) in [
        ("dsh-review-checklist", "DSH"),
        ("pi-review-checklist", "Pi"),
    ] {
        entries.push(Extension {
            id,
            engine,
            name: "代码审阅清单",
            version: "1.0.0",
            kind: "skill",
            description: "检查正确性、用户数据与最小验证的本地 Skill。",
            source: "栖点内置源码",
            web_support: "通过上游 Skills 目录加载，无独立面板",
            scripts: "纯 Markdown，无安装或运行脚本",
        });
    }
    entries
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionRef {
    #[serde(default,skip_serializing_if="std::collections::BTreeMap::is_empty")]
    pub resource_rules: std::collections::BTreeMap<String,Vec<String>>,
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub disabled_resources: Vec<String>,
    pub id: String,
    pub version: String,
    pub enabled: bool,
}

impl ExtensionRef {
    pub fn validate_resources(&self)->Result<()> {
        for (kind,rules) in &self.resource_rules {
            if !self.id.starts_with("npm:") || !["extensions","skills","prompts","themes"].contains(&kind.as_str()) || self.disabled_resources.contains(kind) || rules.len()>256 || rules.iter().any(|rule| {
                let path=rule.trim_start_matches(['!','+','-']);
                path.is_empty() || path.len()>512 || path.starts_with('/') || path.contains('\\') || path.contains(':') || path.split('/').any(|part|part=="..") || path.chars().any(char::is_control)
            }) {return Err(Failure::new("RESOURCE_SELECTION","Pi 文件选择须为包内相对规则，不能与整类禁用重复或引用包外路径"));}
        }
        if !self.disabled_resources.is_empty() && (!self.id.starts_with("npm:") || self.disabled_resources.len()>4 || self.disabled_resources.iter().any(|kind|!["extensions","skills","prompts","themes"].contains(&kind.as_str()))) {return Err(Failure::new("RESOURCE_SELECTION","包内资源选择只适用于 Pi 包的扩展、Skill、提示词和主题"));}
        Ok(())
    }
}

/// Portable recipes contain no paths, connection IDs, credentials or executable URLs.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackOrigin { pub id:String, pub revision:u32 }

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct WorkshopRef {
    #[serde(default,skip_serializing_if="serde_json::Value::is_null")]
    pub requires:serde_json::Value,
    pub kind:String,
    pub id:String,
    pub version:String,
    pub repository:String,
    pub commit:String,
    pub content_path:String,
    pub files:std::collections::BTreeMap<String,String>,
}

#[cfg(test)]
mod file_rule_tests {
    use super::*;
    #[test]
    fn portable_file_rules_preserve_order_and_reject_external_paths() {
        let value=serde_json::json!({"id":"npm:fixture","version":"1.0.0","enabled":true,"resourceRules":{"prompts":["prompts/*.md","!prompts/private.md","+prompts/review.md"]}});
        let item:ExtensionRef=serde_json::from_value(value.clone()).unwrap();
        item.validate_resources().unwrap();
        assert_eq!(serde_json::to_value(&item).unwrap(),value);
        let legacy:ExtensionRef=serde_json::from_value(serde_json::json!({"id":"npm:fixture","version":"1.0.0","enabled":true})).unwrap();
        assert!(legacy.resource_rules.is_empty());
        for invalid in ["../outside","!../outside","C:/private","/private","\\\\host\\private",""] {
            let mut changed=item.clone();changed.resource_rules.insert("prompts".into(),vec![invalid.into()]);
            assert!(changed.validate_resources().is_err(),"{invalid}");
        }
        let mut changed=item.clone();changed.disabled_resources.push("prompts".into());assert!(changed.validate_resources().is_err());
        changed=item.clone();changed.id="dsh-npm:fixture".into();assert!(changed.validate_resources().is_err());
    }
}
impl WorkshopRef {
    pub fn validate(&self)->Result<()> {
        let invalid=||Failure::new("WORKSHOP_REFERENCE","工坊声明必须包含公开仓库、固定提交和安全文件路径及摘要");
        let parts=self.repository.split('/').collect::<Vec<_>>();
        if !["skins","pets","presets"].contains(&self.kind.as_str())||!crate::pack_archive::safe_name(&self.id)||self.id.contains('/')||self.version.is_empty()||self.version.len()>80||self.version.chars().any(char::is_control)||parts.len()!=2||parts.iter().any(|part|part.is_empty()||*part=="."||*part==".."||!part.bytes().all(|b|b.is_ascii_alphanumeric()||b"._-".contains(&b)))||self.commit.len()!=40||!self.commit.bytes().all(|b|b.is_ascii_hexdigit())||(self.content_path!="."&&!crate::pack_archive::safe_name(&self.content_path))||self.files.is_empty()||self.files.len()>2000{return Err(invalid());}
        let mut seen=HashSet::new();
        for (path,hash) in &self.files {
            if !crate::pack_archive::safe_name(path)||path.eq_ignore_ascii_case(".perch-resource.json")||!seen.insert(path.to_lowercase())||hash.len()!=64||!hash.bytes().all(|b|b.is_ascii_hexdigit()){return Err(invalid());}
        }
        let entry=match self.kind.as_str(){"skins"=>"skin.json","pets"=>"pet.json",_=>"preset.yml"};
        if !self.files.contains_key(entry){return Err(invalid());}Ok(())
    }
}
#[cfg(test)]
mod workshop_reference_tests {
 use super::*;
 #[test]
 fn portable_workshop_reference_excludes_machine_fields_and_conflicts(){
  let valid=serde_json::json!({"kind":"skins","id":"sample","version":"1","repository":"example/skins","commit":"0123456789012345678901234567890123456789","contentPath":"skins","files":{"skin.json":"a".repeat(64)}});
  let reference:WorkshopRef=serde_json::from_value(valid.clone()).unwrap();reference.validate().unwrap();
  for field in ["apiKey","connectionId","localPath"]{let mut value=valid.clone();value[field]="private".into();assert!(serde_json::from_value::<WorkshopRef>(value).is_err());}
  let mut changed=reference.clone();changed.content_path="C:/private".into();assert!(changed.validate().is_err());
  changed=reference.clone();changed.files.insert("../escape".into(),"a".repeat(64));assert!(changed.validate().is_err());
  let mut recipe=packs().remove(0);recipe.workshop=vec![reference.clone(),reference];assert!(recipe.validate_metadata().is_err());
 }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Recipe {
    #[serde(default,skip_serializing_if="Option::is_none")]
    pub thinking_level:Option<String>,
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    pub workshop:Vec<WorkshopRef>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cover: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<PackOrigin>,
    pub schema_version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_note: String,
    pub engine: String,
    pub profile_id: String,
    pub extensions: Vec<ExtensionRef>,
    pub model_protocols: Vec<String>,
}

impl Recipe {
    pub fn validate_metadata(&self) -> Result<()> {
        if self.thinking_level.as_ref().is_some_and(|value|!["off","low","high","max"].contains(&value.as_str())){
            return Err(Failure::new("INVALID_PUBLIC_PRESET","默认思考强度不受支持"));
        }
        if self.workshop.len()>128{return Err(Failure::new("WORKSHOP_LIMIT","工坊资源数量超过 128 项"));}
        let mut destinations=HashSet::new();
        for resource in &self.workshop{resource.validate()?;if !destinations.insert(format!("{}/{}",resource.kind,resource.id).to_lowercase()){return Err(Failure::new("WORKSHOP_CONFLICT","工坊资源目标重复，请保留一个版本"));}}
        if !["","developer","researcher","minimal","explorer"].contains(&self.cover.as_str()){return Err(Failure::new("INVALID_PACK_COVER","请选择应用内提供的整合包封面"));}
        if self.origin.as_ref().is_some_and(|origin|uuid::Uuid::parse_str(&origin.id).is_err()||origin.revision==0){return Err(Failure::new("INVALID_PACK_ORIGIN","组合来源修订无效"));}
        for (value,limit) in [(&self.description,1000),(&self.source_note,2000)] {
            if value.chars().count()>limit || value.chars().any(|c|c.is_control() && c!='\n' && c!='\r' && c!='\t') {
                return Err(Failure::new("INVALID_PACK_DESCRIPTION","组合简介或来源说明过长，或包含不支持的字符"));
            }
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_metadata()?;
        if !self.workshop.is_empty()&&self.engine!="DSH"{return Err(Failure::new("WORKSHOP_ENGINE","工坊资源只能用于 DSH 组合"));}
        if self.schema_version != 1
            || self.name.trim().is_empty()
            || self.name.chars().count() > 80
            || self.name.chars().any(char::is_control)
        {
            return Err(Failure::new(
                "INVALID_RECIPE",
                "整合包名称或清单版本不受支持",
            ));
        }
        let profile=engine_profile::resolve(&self.engine, &self.profile_id)?;
        let package:serde_json::Value=serde_json::from_str(&profile.package)?;
        if self.extensions.len() > 16 || self.model_protocols.is_empty() {
            return Err(Failure::new(
                "INVALID_RECIPE",
                "扩展数量或模型协议声明不正确",
            ));
        }
        for protocol in &self.model_protocols {
            if !["deepseek", "openai-chat", "openai-responses", "anthropic"]
                .contains(&protocol.as_str())
            {
                return Err(Failure::new("INVALID_PROTOCOL", "整合包声明了未知模型协议"));
            }
            crate::store::compatible(&self.engine, protocol)?;
        }
        let mut seen = HashSet::new();
        for item in &self.extensions {
            item.validate_resources()?;
            if let Some(name)=item.id.strip_prefix("npm:").or_else(||item.id.strip_prefix("dsh-npm:")){
                if ["@deepseek-ai/dsh","@agegr/pi-web","@earendil-works/pi-coding-agent","@mariozechner/pi-coding-agent"].contains(&name){return Err(Failure::new("HOST_AS_EXTENSION","引擎和工作台属于版本组合，不能作为扩展加载；请从扩展清单移除此项，版本组合保留"));}
                if self.engine!=(if item.id.starts_with("dsh-npm:"){"DSH"}else{"Pi"})||!engine_profile::valid_package_name(name)||package["dependencies"][name].as_str()!=Some(item.version.as_str())||!seen.insert(&item.id){return Err(Failure::new("PACKAGE_LOCK","外部包必须在此组合的精确依赖锁中，不能跨生态装配"));}
                continue;
            }
            if crate::local_skills::valid_id(&item.id){
                if item.version!="1"||!seen.insert(&item.id){return Err(Failure::new("SKILL_VERSION","本地 Skill 版本或重复项无效"));}
                continue;
            }
            let entry = extensions()
                .into_iter()
                .find(|e| e.id == item.id)
                .ok_or_else(|| {
                    Failure::new(
                        "EXTENSION_UNVERIFIED",
                        "外部或未知扩展尚未验证，当前不执行其代码",
                    )
                })?;
            if entry.engine != self.engine {
                return Err(Failure::new(
                    "EXTENSION_ECOSYSTEM",
                    "DSH 插件与 Pi 扩展不能互装",
                ));
            }
            if !(item.version == "1.0.0" || (entry.kind == "tool" && item.version == "1.1.0"))
                || !seen.insert(&item.id)
            {
                return Err(Failure::new(
                    "EXTENSION_CONFLICT",
                    "扩展版本不可用或清单包含重复扩展",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn validate_workshop_hosts(&self)->Result<()> {
        for resource in &self.workshop {
            let host=match resource.kind.as_str(){"skins"=>"@linxin666/dsh-client-ui-skin-center","pets"=>"@linxin666/dsh-pet","presets"=>"@linxin666/dsh-client-ui-preset-center",_=>return Err(Failure::new("WORKSHOP_KIND","工坊资源类型无效"))};
            if self.engine!="DSH"||!self.extensions.iter().any(|item|item.enabled&&(item.id==format!("dsh-npm:{host}")||item.id=="dsh-npm:@linxin666/dsh-web-all")){
                return Err(Failure::new("WORKSHOP_HOST",&format!("资源 {} 缺少已启用的宿主 {host}；可先保存草稿",resource.id)));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod public_preset_tests {
    use super::*;
    #[test]
    fn public_thinking_preset_roundtrip_and_allowlist() {
        let mut recipe=packs().remove(0);
        let legacy=serde_json::to_value(&recipe).unwrap();
        assert!(legacy.get("thinkingLevel").is_none());
        assert_eq!(serde_json::from_value::<Recipe>(legacy).unwrap().thinking_level,None);
        for level in ["off","low","high","max"] {
            recipe.thinking_level=Some(level.into());
            recipe.validate().unwrap();
            let exported=serde_json::to_vec(&recipe).unwrap();
            assert_eq!(serde_json::from_slice::<Recipe>(&exported).unwrap(),recipe);
        }
        recipe.thinking_level=Some("secret-or-path".into());
        assert!(recipe.validate_metadata().is_err());
    }
}

pub fn packs() -> Vec<Recipe> {
    [
        (&engine_profile::DSH, "DSH 日常开发", "dsh-local-clock"),
        (&engine_profile::PI, "Pi 轻量开发", "pi-local-clock"),
    ]
    .into_iter()
    .map(|(p, name, id)| Recipe {
        thinking_level:None,
        workshop:Vec::new(),
        cover: String::new(),
        origin: None,
        description: if p.engine == "Pi" {
            "使用 Pi Web 工作台，面向希望沿用 Pi 扩展生态的用户；附本地时间工具与代码审阅清单。"
        } else {
            "使用 DeepSeek Harness 工作台，面向希望沿用 DSH 插件生态的用户；附本地时间工具与代码审阅清单。"
        }.into(),
        source_note: String::new(),
        schema_version: 1,
        name: name.into(),
        engine: p.engine.into(),
        profile_id: p.id.into(),
        extensions: vec![
            ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),
                id: id.into(),
                version: "1.1.0".into(),
                enabled: true,
            },
            ExtensionRef{resource_rules:Default::default(),disabled_resources:Vec::new(),
                id: if p.engine == "Pi" {
                    "pi-review-checklist"
                } else {
                    "dsh-review-checklist"
                }
                .into(),
                version: "1.0.0".into(),
                enabled: true,
            },
        ],
        model_protocols: vec!["deepseek".into(), "openai-chat".into()],
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_recipes_reject_foreign_code_and_private_fields() {
        for recipe in packs() {
            recipe.validate().unwrap();
        }
        let mut recipe = packs().remove(0);
        recipe.extensions[0].id = "pi-local-clock".into();
        assert_eq!(recipe.validate().unwrap_err().code, "EXTENSION_ECOSYSTEM");
        recipe.extensions[0].id = "https://unknown.example/plugin.js".into();
        assert_eq!(recipe.validate().unwrap_err().code, "EXTENSION_UNVERIFIED");
        let mut document = serde_json::to_value(packs().remove(0)).unwrap();
        document["apiKey"] = "secret".into();
        assert!(serde_json::from_value::<Recipe>(document).is_err());
    }
}
