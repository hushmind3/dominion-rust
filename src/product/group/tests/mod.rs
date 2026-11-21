
    use super::*;

    fn projected_text(context:&RoomContext)->String{
        context.messages.iter().filter_map(|message|message["content"].as_str())
            .collect::<Vec<_>>().join("\n")
    }


mod conversation;
mod remote;
