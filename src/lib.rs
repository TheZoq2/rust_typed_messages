
/*!
    Module for sending and receiving json serialised classes over sockets
*/


extern crate serde;
extern crate serde_json;

#[macro_use]
extern crate error_chain;

use std::net::{TcpStream, TcpListener};

use serde::{Serialize};
use serde::de::{DeserializeOwned};

use std::{io};
use std::time::Duration;

use std::io::prelude::*;



///Byte which marks the end of a json message
const MESSAGE_END_MARKER: u8 = 1;


////////////////////////////////////////////////////////////////////////////////
//                          Error struct
////////////////////////////////////////////////////////////////////////////////
error_chain! {
    foreign_links {
        ReadFail(io::Error);
        JsonError(serde_json::Error);
        Utf8Error(std::string::FromUtf8Error);
    }
}

////////////////////////////////////////////////////////////////////////////////

fn read_string_from_stream_until_end_marker<T: io::Read>(stream: &mut T) -> Result<String>
{
    const BUFFER_SIZE: usize = 128;
    let mut bytes = vec!();

    'outer: loop
    {
        //Read one byte from the stream
        let mut buffer: [u8; BUFFER_SIZE] = [0; BUFFER_SIZE];
        stream.read(&mut buffer)?;

        for byte in buffer.iter()
        {
            if *byte == MESSAGE_END_MARKER
            {
                break 'outer;
            }
            else if *byte != 0
            {
                bytes.push(*byte);
            }
            else
            {
                break;
            }
        }
    }

    Ok(String::from_utf8(bytes)?)
}
fn string_to_bytes_with_end_marker(string: String) -> Vec<u8>
{
    let mut bytes = string.into_bytes();
    bytes.push(MESSAGE_END_MARKER);
    bytes
}

/**
  Replies to a single message of MsgType with a message of ReplyType using
  the reply_handler function
*/
pub fn handle_read_reply_client<MsgType, ReplyType, Function, InputType>
                (ref mut reply_handler: Function, stream: &mut InputType)
        -> Result<()>
    where MsgType: Serialize + DeserializeOwned, 
          ReplyType: Serialize + DeserializeOwned,
          Function: FnMut(MsgType) -> ReplyType,
          InputType: Read + Write
{
    //stream.read_to_string(&mut buffer)?;
    let buffer = read_string_from_stream_until_end_marker(stream)?;

    //Decode the message. If the message is not of the specified type, this fails.
    let decoded = serde_json::from_str(&buffer)?;

    //Run the reply handler to get a reply
    let reply = reply_handler(decoded);

    //Encode the result and send it back
    let encoded = serde_json::to_string(&reply)?;
    //stream.write_all(&encoded.into_bytes())?;
    stream.write(&string_to_bytes_with_end_marker(encoded))?;

    Ok(())
}

/**
  Creates a TCP listener that listens for messages of a certain type, and replies with messages
  of another type by running the reply_handler on those messages
 */
pub fn run_read_reply_server<MsgType, ReplyType, Function>(
            port: u16,
            mut reply_handler: Function,
            timeout: Option<Duration>
        )
        -> Result<()>
    where MsgType: Serialize + DeserializeOwned, 
          ReplyType: Serialize + DeserializeOwned,
          Function: FnMut(MsgType) -> ReplyType
{
    let address: &str = &format!("127.0.0.1:{}", port);
    //let tcp_listener = TcpListener::bind(&format!("127.0.0.1:80", port))?;
    let tcp_listener = TcpListener::bind(address)?;


    for stream in tcp_listener.incoming()
    {
        let mut stream = stream?;
        stream.set_read_timeout(timeout).unwrap();
        stream.set_write_timeout(timeout).unwrap();

        handle_read_reply_client(&mut reply_handler, &mut stream)?;
    }

    Ok(())
}



/**
    Connects to a remote tcp socket, sends a message, waits for a reply
    and returns that reply
*/
pub fn connect_send_read<MsgType, ReplyType>(ip: &str, port: u16, msg: MsgType, timeout: Option<Duration>)
        -> Result<ReplyType>
    where MsgType: Serialize + DeserializeOwned, ReplyType: Serialize + DeserializeOwned
{
    let address: &str = &format!("{}:{}", ip, port);
    let mut stream = TcpStream::connect(address)?;
    stream.set_read_timeout(timeout).unwrap();
    stream.set_write_timeout(timeout).unwrap();

    send_message_read_reply::<_, ReplyType, _>(msg, &mut stream)
}

/**
    Sends a message to an IO stream
*/
pub fn send_message_read_reply<MsgType, ReplyType, IOType>(msg: MsgType, io_stream: &mut IOType)
        -> Result<ReplyType>
    where 
        MsgType: Serialize + DeserializeOwned, 
        ReplyType: Serialize + DeserializeOwned,
        IOType: Read + Write
{
    //Encode the message as json
    let encoded = serde_json::to_string(&msg).unwrap();
    let encoded_as_string = encoded.to_string();

    //Send it through the socket
    io_stream.write_all(&string_to_bytes_with_end_marker(encoded_as_string))?;

    //io_stream.read_to_string(&mut buffer)?;
    let buffer = read_string_from_stream_until_end_marker(io_stream)?;

    let decoded = serde_json::from_str(&buffer)?;
    Ok(decoded)
}




#[cfg(test)]
mod json_socket_tests
{
    use super::*;
    use super::MESSAGE_END_MARKER;
    use super::string_to_bytes_with_end_marker;


    use std::io::{Read, Write};
    use std::io;

    use serde_json;

    struct ReaderWriterDummy
    {
        ///Dummy buffer that is read from
        read_buffer: Vec<u8>, 
        ///Dummy buffer that is written to
        write_buffer: Vec<u8>, 
    }

    impl ReaderWriterDummy
    {
        pub fn new(mut read_content: Vec<u8>) -> Self 
        {
            read_content.reverse();

            ReaderWriterDummy {
                read_buffer: read_content,
                write_buffer: vec!()
            }
        }
        fn get_written(&self) -> &Vec<u8>
        {
            &self.write_buffer
        }
    }

    impl Read for ReaderWriterDummy
    {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>
        {
            match self.read_buffer.pop()
            {
                Some(val) => {
                    buf[0] = val;
                    Ok(1)
                },
                None => Ok(0)
            }
        }
    }
    impl Write for ReaderWriterDummy
    {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize>
        {
            for elem in buf
            {
                self.write_buffer.push(*elem);
            }

            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()>
        {
            Ok(())
        }
    }

    #[test]
    fn meta_read_tests()
    {
        let mut dummy = ReaderWriterDummy::new(Vec::from("56".as_bytes()));

        let mut buffer = String::new();
        dummy.read_to_string(&mut buffer).unwrap();
        assert_eq!(buffer, "56");
    }

    #[test]
    fn meta_write_test()
    {
        let mut dummy = ReaderWriterDummy::new(vec!());

        let buffer = String::from("yoloswag");
        dummy.write_all(&buffer.into_bytes()).unwrap();

        let written = dummy.get_written().clone();

        println!("{}", written.len());

        assert_eq!(
                String::from_utf8(written).unwrap(), 
                String::from("yoloswag")
            );
    }

    #[test]
    fn meta_read_end_of_file()
    {
        let mut dummy = ReaderWriterDummy::new(string_to_bytes_with_end_marker(String::from("yolo")));

        let mut expected = String::from("yolo").into_bytes();
        expected.push(MESSAGE_END_MARKER);

        let mut buffer = vec!();
        dummy.read_to_end(&mut buffer).unwrap();
        assert_eq!(buffer, expected);
    }

    #[test]
    fn end_of_stream_tests()
    {
        let mut expected = String::from("yoloswag").into_bytes();
        expected.push(MESSAGE_END_MARKER);

        assert_eq!(string_to_bytes_with_end_marker(String::from("yoloswag")), expected);
    }

    #[test]
    fn send_read_test()
    {
        let json_encoded = serde_json::to_string(&56).unwrap();

        //Create a dummy buffer containing 56
        let mut dummy = ReaderWriterDummy::new(string_to_bytes_with_end_marker(json_encoded));

        assert!(send_message_read_reply::<i32, i32, ReaderWriterDummy>(5, &mut dummy).unwrap() == 56);
    }

    #[test]
    fn response_function_test()
    {
        let response_function = |x: i32|{
            x * x
        };

        let mut dummy = ReaderWriterDummy::new(string_to_bytes_with_end_marker(serde_json::to_string(&10).unwrap()));

        assert!(handle_read_reply_client(&response_function, &mut dummy).is_ok());
        assert!(
                dummy.get_written() == 
                &string_to_bytes_with_end_marker(serde_json::to_string(&100).unwrap())
            );
    }

    #[test]
    fn modify_outer_test()
    {
        let mut buffer = 0;
        {
            let response_function = |x|{buffer = x};

            let mut dummy = ReaderWriterDummy::new(string_to_bytes_with_end_marker(serde_json::to_string(&10).unwrap()));

            handle_read_reply_client(response_function, &mut dummy).is_ok();
        }

        assert!(buffer == 10);
    }
}

