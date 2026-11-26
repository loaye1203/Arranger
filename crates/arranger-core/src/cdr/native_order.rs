//! Recover cross-owner interleaving from existing native StaticID scalars.
//! A changed native sibling order or unbound new object always takes priority.
use crate::Document;
use std::collections::{BTreeMap,BTreeSet};
pub(super) fn restore(doc:&mut Document,native_ids:&BTreeMap<String,u32>,known:&BTreeSet<String>,source_indices:&BTreeMap<String,u32>)->bool {
    // Corel reallocates StaticIDs inside PowerClips on ordinary SaveAs.
    // A scalar source index retains cross-owner serialization interleaving;
    // native owners, hierarchy and each sibling ordering still take priority.
    let ids=if doc.objects.iter().all(|o|source_indices.contains_key(&o.id)){source_indices}else{native_ids};
    if known.is_empty(){return false;}let mut seen=BTreeSet::new();let mut previous=BTreeMap::new();
    for object in &doc.objects {
        if !known.contains(&object.id){return false;}let Some(id)=ids.get(&object.id).copied().filter(|id|*id>0)else{return false};
        if !seen.insert(id){return false;}
        let bucket=(&object.page_id,&object.parent_id);
        if previous.insert(bucket,id).is_some_and(|before|before>=id){return false;}
    }
    let changed=doc.objects.windows(2).any(|pair|ids[&pair[0].id]>ids[&pair[1].id]);
    if changed{doc.objects.sort_by_key(|o|ids[&o.id]);}changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn native_bring_to_front_and_new_objects_are_never_undone() {
        let mut doc:Document=serde_json::from_value(json!({"id":"order-edit","unit":"mm","pages":[{"id":"page-1","width_mm":210.,"height_mm":297.}],"resources":{},"objects":[]})).unwrap();
        for(id,page)in[("last","page-1"),("first","page-1"),("shared","_workspace")]{doc.objects.push(serde_json::from_value(json!({"id":id,"page_id":page,"x_mm":10.,"y_mm":20.,"width_mm":12.,"height_mm":8.,"geometry":{"kind":"rect","radius_mm":0.}})).unwrap());}
        let ids=BTreeMap::from([("first".into(),1),("shared".into(),2),("last".into(),3)]);let known=ids.keys().cloned().collect();let expected=doc.clone();assert!(!restore(&mut doc,&ids,&known,&ids));assert_eq!(doc.objects,expected.objects);
        doc.objects.swap(0,1);let mut partial=known.clone();partial.remove("shared");let expected=doc.clone();assert!(!restore(&mut doc,&ids,&partial,&ids));assert_eq!(doc.objects,expected.objects);
    }
    #[test]
    fn reallocated_native_ids_do_not_lose_cross_owner_serialization_indices() {
        let mut doc=Document::new("reallocated".into(),210.,297.).unwrap();
        for(id,owner)in[("first","page-1"),("last","page-1"),("shared","_workspace")]{doc.objects.push(serde_json::from_value(json!({"id":id,"page_id":owner,"x_mm":0.,"y_mm":0.,"width_mm":10.,"height_mm":10.,"geometry":{"kind":"rect","radius_mm":0.}})).unwrap());}
        let indices=BTreeMap::from([("first".into(),1),("shared".into(),2),("last".into(),3)]);let known=indices.keys().cloned().collect();let native=BTreeMap::from([("first".into(),31),("last".into(),30),("shared".into(),17)]);
        assert!(restore(&mut doc,&native,&known,&indices));assert_eq!(doc.objects.iter().map(|o|o.id.as_str()).collect::<Vec<_>>(),vec!["first","shared","last"]);
    }
}
