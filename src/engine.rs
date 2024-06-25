use std::{
    io::{
        self,
        stdin,
        Write,
    },
    collections::HashSet,
    fmt,
};

use crate::{
    error_code::ErrorCode,
    flag::Flag,
    variable_metadata::{
        // VariableMetadata,
        VariableSize,
    },
    line_processor::LineProcessor,
    memory_manager::MemoryManager,
    register::{
        get_register, 
        Register,
        get_register_size
    },
    // status::Status,
    utils::{
        read_lines_from_file, 
        parse_string_to_usize
    },
    instruction::Instruction
};

const MEMORY_SIZE: usize = 1024 * 16; // 16 KB

fn skip_lines(lines_to_skip: usize) -> io::Result<()> {
    let mut stdout = io::stdout();
    // Move cursor up by `lines_to_skip` lines without clearing them
    for _ in 0..lines_to_skip {
        write!(stdout, "\x1B[B")?;  // ANSI code to move cursor up one line
    }

    // Flush output to ensure everything is printed
    stdout.flush()?;

    Ok(())
}


fn clear_screen(rows: usize) -> io::Result<()> {
    let mut stdout = io::stdout();

    // Move the cursor to the beginning of the terminal
    write!(stdout, "\x1B[H")?;

    // Clear the first N rows
    for row in 0..rows {
        // Move the cursor to the beginning of the current row
        write!(stdout, "\x1B[{};0H", row + 1)?;
        // Clear the current line
        write!(stdout, "\x1B[2K")?;
    }
    write!(stdout, "\x1B[H")?; // Return to the beginning of the terminal
    // Make sure all commands are executed
    stdout.flush()?;

    Ok(())
}

fn pause() {
    let mut s = String::new();
    let _ = stdin().read_line(&mut s);
}

fn back_to_str(vec: &Vec<String>) -> String {
    let mut ret = String::new();

    ret.push_str(&format!("{} ", vec[0]));

    for i in 1..vec.len() {
        ret.push_str(&format!("{}, ", vec[i]));
    }
    let f = if vec.len() == 1 {1} else {2};
    ret[..ret.len()-f].to_string()
}

fn combine_parts(vec: &Vec<String>) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    let mut combined: String = String::new();
    let mut in_quotes: bool = false;

    for item in vec {
        if item.starts_with('\'') && item.ends_with('\'') {
            result.push(format!("\'{}\'", item.trim_matches('\'').to_string()));
        } else if item.starts_with('\'') {
            combined.push_str(&format!("\'{}", item.trim_matches('\'').to_string()));
            in_quotes = true;
        } else if item.ends_with('\'') {
            combined.push_str(", ");
            combined.push_str(&format!("{}\'", item.trim_matches('\'').to_string()));
            result.push(combined.clone());
            combined.clear();
            in_quotes = false;
        } else if in_quotes {
            combined.push_str(", ");
            combined.push_str(&item);
        } else {
            result.push(item.to_string());
        }
    }

    // If there's a trailing combined string without a closing quote
    if !combined.is_empty() {
        result.push(combined);
    }

    result
}

pub struct Engine {
    lines: LineProcessor, // lines of source code (.txt)
    pub registers: [Register; 10], // A-D, ESI, EDI, P
    memory_manager: MemoryManager, // 16 KB bytes of memory
    // mode: bool, // false = reading data, true = reading code
    // status: Status, // status: ok, error, halted,
    valid_registers: HashSet<String>,
    // interrupts: Vec<Interrupt>
}


impl Engine {
    pub fn new(file_name: &str) -> io::Result<Self> {
        let file_lines = read_lines_from_file(file_name)?;
        let my_registers: [Register; 10] = [
            Register::new("EAX"),
            Register::new("EBX"),
            Register::new("ECX"),
            Register::new("EDX"),
            Register::new("ESI"),
            Register::new("EDI"),
            Register::new("BP"),
            Register::new("SP"),
            Register::new("IP"),
            Register::new("FLAG"),
        ];

        let ds = 0 as usize;         // DATA SEGMENT starts at 0
        let cs = 1024 * 3 as usize;  // CODE SEGMENT starts at 3072 
        let ss: usize = MEMORY_SIZE - 1024; // STACK SEGMENT, starts at 15360 (1024*15)

        let valid_registers: HashSet<String> = [         
            "AX", "BX", "CX", "DX", "SI", "DI", "IP", "FLAG", "BP", "SP",
            "AL", "AH", "BL", "BH", "CL", "CH", "DL", "DH",
            "EAX", "EBX", "ECX", "EDX", "ESI", "EDI",  "EBP", "ESP"
        ].iter().cloned().map(String::from).collect();
            Ok(Self {
                lines: LineProcessor::new(file_lines),
                registers: my_registers,
                memory_manager: MemoryManager::new(MEMORY_SIZE, [ds, cs, ss]),
                valid_registers,
            })
    }

    fn is_valid_register(&self, reg_to_check: &str) -> bool {
        self.valid_registers.contains(reg_to_check)
    }
    // Only used for tests
    pub fn get_memory(&self, amount: usize) -> Vec<u8> {
        self.memory_manager._get_memory(0, amount)
    }

    pub fn get_register_value(&self, name: &str) -> Result<u32, ErrorCode> {
        if !self.is_valid_register(name) {
            return Err(ErrorCode::InvalidRegister(name.to_string()));
        }
        let value = self.registers[get_register(name)].get_dword();
        match name {
            "AL"   | "BL"  | "CL" | "DL" => Ok(value & 0x000000FF),
            "AH"   | "BH"  | "CH" | "DH" => Ok((value & 0x0000FF00) >> 8),
            "FLAG" | "AX"  | "BX" | "CX"  | "DX" | "SI" | "DI" | "SP"  => Ok(value & 0x0000FFFF), 
            "ESI"  | "EDI" | "IP" | "EAX" | "EBX" | "ECX" | "EDX"  => Ok(value), 
            _ => Err(ErrorCode::InvalidRegister(name.to_string()))
        }
    }
    
    fn set_flags(&mut self, result: usize, size: VariableSize, overflowed: bool) {

        self.set_register_value("FLAG", 0).expect("Flag Register Missing?!");

        if result.count_ones() % 2 == 0 {
            self.set_flag(Flag::Parity, true);
        }

        // Set Zero Flag
        if result == 0 {
            self.set_flag(Flag::Zero, true);    
        }

        // Set Sign Flag
        let mask = (0xFF as usize) << (size.value() - 1) * 8;
        let displacement = size.value() * 8 - 1;
        if (result & mask) >> displacement == 1 {
            self.set_flag(Flag::Sign, true);    
        }

        // Set Overflow Flag
        if overflowed {
            self.set_flag(Flag::Overflow, true);    
        }
        
    }



    fn get_pointer_argument_size<'a>(&self, argument: &'a str) -> (Option<VariableSize>, &'a str) {
        let parts: Vec<&str> = argument.split_whitespace().collect();
        match parts.as_slice() {
            ["BYTE", "PTR", arg] => (Some(VariableSize::Byte), *arg),
            ["WORD", "PTR", arg] => (Some(VariableSize::Word), *arg),
            ["DWORD", "PTR", arg] => (Some(VariableSize::DoubleWord), *arg),
            [arg] => {
                if let Ok((_, size)) = self.parse_value_from_parameter(*arg, None) {
                    (Some(size), *arg)
                } else {
                    (None, *arg)
                }
            }
            _ => (None, argument)
        }
    }

    pub fn parse_value_from_parameter(&self, parameter: &str, mut size: Option<VariableSize>) -> Result<(u32, VariableSize), ErrorCode> {
        if let Some(value) = parse_string_to_usize(parameter){
            // Constant
            let vi32 = value as i32;
            let assumed_size = if vi32 >= i8::MIN.into() || vi32 <= u8::MAX.into() {
                VariableSize::Byte
            } else if vi32 >= i16::MIN.into() || vi32 <= u16::MAX.into() {
                VariableSize::Word
            } else if vi32 >= i32::MIN.into() || value <= u32::MAX {
                VariableSize::DoubleWord
            } else {
                return Err(ErrorCode::InvalidValue(format!("value {} is outside of allowed range. 0-u32::MAX", value)));
            };

            Ok((value as u32, assumed_size))

        } else if let Ok(value) = self.get_register_value(parameter) {
            // Register
            let assumed_size = match parameter {
                "AL"|"AH"|"BL"|"BH"|"CL"|"CH"|"DL"|"DH" => {
                    VariableSize::Byte
                },
                "EAX"|"EBX"|"ECX"|"EDX"|"ESI"|"EDI" => {
                    VariableSize::DoubleWord
                },
                "AX"|"BX"|"CX"|"DX"|"SI"|"DI"|"FLAG"|"IP"|"SP"  => {
                    VariableSize::Word
                },
                _ => return Err(ErrorCode::InvalidRegister(parameter.to_string())) // Unreachable
            };
            Ok((value, assumed_size))
        } else if let Ok(parsed_address) = self.memory_manager.calculate_effective_address(parameter, &self.registers,  true) {
   
            // Memory (adjust size to pointer size)
            let memory_address_str = parameter.to_string();
            if memory_address_str.len() < 3 {
                return Err(ErrorCode::InvalidPointer("Pointer length must be more than 3 including []".to_string()));
            }
            if size.is_none() {
                if let Some(metadata) = self.memory_manager.get_variable(&memory_address_str[1..memory_address_str.len()-1]) {
                    size = Some(metadata.size);
                } else if let Some(_) = self.memory_manager.labels.get(&memory_address_str[1..memory_address_str.len()-1]) {
                    size = Some(VariableSize::Word);
                }
                
            }

            match size {
                Some(VariableSize::Byte)|None => {
                    let a = self.memory_manager.get_byte(parsed_address)?;
                    Ok((a as u32, VariableSize::Byte))
                },
                Some(VariableSize::Word) => {
                    let a = self.memory_manager.get_word(parsed_address)?;
                    Ok((a as u32, VariableSize::Word))
                },
                Some(VariableSize::DoubleWord) => {
                    let a = self.memory_manager.get_dword(parsed_address)?;
                    Ok((a as u32, VariableSize::DoubleWord))
                },
            }
        } else {
            Err(ErrorCode::InvalidValue(format!("Parameter {parameter} could not be parsed.")))
        }
    }

    pub fn execute(&mut self, debug: bool) -> Result<(), ErrorCode>{
        // Go over code to get all the labels.
        let mut in_proc = false;
        let mut current_proc_start_ip: usize = 0;
        let mut current_proc_name = String::new();
        let mut lines_to_skip = 1;
        loop {
            let line_option = self.lines.next();
            if line_option.is_none() {
                break;
            }

            if let Some(line) = line_option {
                let proc_string = "PROC".to_string();
                let end_proc_string = "END".to_string();
                match line.as_slice() {
                    [label] if label.ends_with(":") && self.memory_manager.is_valid_variable_name(&label[..label.len()-1]) => {
                        let no_colon = label[..label.len()-1].to_string();
                        if self.memory_manager.labels.get(&no_colon).is_some() {
                            return Err(ErrorCode::LabelAlreadyExists(format!("Label {no_colon} already exists")));
                        }
                        if let Err(error) = self.memory_manager.save_label(no_colon, self.get_register_value("IP")? as usize) {
                            return Err(error);
                        }
                    },
                    [proc_name, proc] if proc == &proc_string => {
                        if self.memory_manager.procs.get(proc_name).is_some() {
                            return Err(ErrorCode::LabelAlreadyExists(format!("Proc {proc_name} already exists")));
                        }
                        if in_proc {
                            return Err(ErrorCode::InvalidOpcode(format!("Cannot start new proc while in another.")));
                        }
                        in_proc = true;
                        current_proc_name = proc_name.clone();
                        current_proc_start_ip = self.get_register_value("IP")? as usize;
                    }

                    [proc_name, proc] if proc == &end_proc_string => {
                        if self.memory_manager.procs.get(proc_name).is_some() {
                            return Err(ErrorCode::LabelAlreadyExists(format!("Proc {proc_name} already exists")));
                        }
                        if !in_proc {
                            return Err(ErrorCode::InvalidOpcode(format!("Cannot end proc outside of a proc.")));
                        }
                        in_proc = false;
                        let current_proc_end_ip = self.get_register_value("IP")? as usize;
                        self.memory_manager.procs.insert(current_proc_name.clone(), (current_proc_start_ip+1, current_proc_end_ip+1));
                    }
                    _ => {}
                }
            }
            self.lines.update_ip_register(&mut self.registers[get_register("IP")]);
        }
        
        self.lines.set_ip(0);
        self.lines.update_ip_register(&mut self.registers[get_register("IP")]);
        let mut buffer: Option<(usize, String)> = None;
        let _ = clear_screen(100);
        loop {
            let line_option = self.lines.next();
            self.lines.update_ip_register(&mut self.registers[get_register("IP")]);
            let ip: usize = self.get_register_value("IP")? as usize;

            if line_option.is_none() {
                break;
            }
            let line = line_option.unwrap();
            if debug {
                let _ = clear_screen(14);
                if let Some((prev_ip, prev_line)) = buffer {
                    println!("[{}]: {}", prev_ip, prev_line); // Previous
                } else {
                    println!("");
                }
                println!("[{}]: {} <- YOU ARE HERE", ip, back_to_str(&line)); // Current
                
                if let Some((next_line, next_ip)) = self.lines.peak() {
                    println!("[{}]: {}", next_ip, back_to_str(&next_line)); // Next
                    buffer = Some((ip, back_to_str(&line)));
                } else {
                    println!("");
                    buffer = None;
                }

                println!("{}", self);

                pause();
            }

            let combining_inside_quotes: Vec<String> = combine_parts(&line);
            let line_str: Vec<&str> = combining_inside_quotes.iter().map(|s| &**s).collect();
            match line_str.as_slice() {
                // INC DEC 
                [op @ ("inc" | "dec"), register] if self.is_valid_register(register) => {
                    let inc = *op == "inc";
                    let (result, overflowed) = match get_register_size(register).unwrap() {
                        VariableSize::Byte => {
                            let (value, overflowing) = if inc {
                                (self.get_register_value(register)? as u8).overflowing_add(1)

                            } else {
                                (self.get_register_value(register)? as u8).overflowing_sub(1)
                            };

                            self.set_register_value(register, value as u32)?;
                            (value as u32, overflowing)
                        },
                        VariableSize::Word => {
                            let (value, overflowing) = if inc {
                                (self.get_register_value(register)? as u16).overflowing_add(1)

                            } else {
                                (self.get_register_value(register)? as u16).overflowing_sub(1)
                            };

                            self.set_register_value(register, value as u32)?;
                            (value as u32, overflowing)
                        },
                        VariableSize::DoubleWord => {
                            let (value, overflowing) = if inc {
                                (self.get_register_value(register)? as u32).overflowing_add(1)

                            } else {
                                (self.get_register_value(register)? as u32).overflowing_sub(1)
                            };

                            self.set_register_value(register, value)?;
                            (value, overflowing)
                        },
                    };
                self.set_flags(result as usize, get_register_size(register).unwrap(), overflowed)
                },
                [op @ ("inc" | "dec"), memory_address] if self.memory_manager.is_memory_operand(memory_address) => {
                                    // Case with WORD/BYTE PTR

                    let (size_option, memory_address_str) = self.get_pointer_argument_size(memory_address);
                    let inc = *op == "inc";
                    let size = size_option.unwrap_or(VariableSize::Byte);
                    // Calculate effective address
                    match self.memory_manager.calculate_effective_address(memory_address_str, &self.registers, true) {
                        Ok(parsed_address) => {
                            let (current, overflowed) = match size {
                                VariableSize::Byte => {
                                    if inc {
                                        let (cu8, o) = self.memory_manager.get_byte(parsed_address)?.overflowing_add(1);
                                        (cu8 as u32, o)
                                    } else {
                                        let (cu8, o) = self.memory_manager.get_byte(parsed_address)?.overflowing_sub(1);
                                        (cu8 as u32, o)
                                    }
                                },
                                VariableSize::Word => {
                                    if inc {
                                        let (cu16, o) = self.memory_manager.get_word(parsed_address)?.overflowing_add(1);
                                        (cu16 as u32, o)
                                    } else {
                                        let (cu16, o) = self.memory_manager.get_word(parsed_address)?.overflowing_sub(1);
                                        (cu16 as u32, o)
                                    }
                                },
                                VariableSize::DoubleWord => {
                                    if inc {
                                        let (cu16, o) = self.memory_manager.get_dword(parsed_address)?.overflowing_add(1);
                                        (cu16 as u32, o)
                                    } else {
                                        let (cu16, o) = self.memory_manager.get_dword(parsed_address)?.overflowing_sub(1);
                                        (cu16 as u32, o)
                                    }
                                }
                            };
                            
                            self.set_flags(current as usize, size, overflowed);
                            
                            match size {
                                VariableSize::Byte => self.memory_manager.set_byte(parsed_address, current as u8)?,
                                VariableSize::Word => self.memory_manager.set_word(parsed_address, current as u16)?,
                                VariableSize::DoubleWord => self.memory_manager.set_dword(parsed_address, current as u32)?,
                            };
                        },
                        Err(error) => return Err(error),
                    }
                },
                ["inc", _rest @ ..] => {
                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;
                    println!("{}", Instruction::get_help_string(Instruction::Inc));
                    return Err(ErrorCode::InvalidOpcode(line.join(", ")));
                },
                ["dec", _rest @ ..] => {
                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;
                    println!("{}", Instruction::get_help_string(Instruction::Dec));
                    return Err(ErrorCode::InvalidOpcode(line.join(", ")));
                },
                // LEA
                ["lea", reg, memory_address] if self.is_valid_register(*reg) && self.memory_manager.is_memory_operand(memory_address) => {
                    match self.memory_manager.calculate_effective_address(memory_address, &self.registers, true) {
                        Ok(parsed_address) => {
                            // Determine the register size
                            match get_register_size(*reg) {
                                Some(VariableSize::Byte) => {
                                    return Err(ErrorCode::NotEnoughSpace(
                                        format!("Cannot store pointer in 1-byte register: {reg}."))
                                    );
                                },
                                Some(VariableSize::Word)|Some(VariableSize::DoubleWord) => {
                                    self.mov_reg_const(reg, parsed_address as u32)?;
                                },
                                _ => {
                                    return Err(ErrorCode::InvalidRegister(reg.to_string()));
                                },
                            }
                        },
                        Err(error) => return Err(error),
                    }
                },
                ["lea", _rest @ ..] => {
                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;
                    println!("{}", Instruction::get_help_string(Instruction::Lea));
                    return Err(ErrorCode::InvalidOpcode(line.join(", ")));
                },
                // MOV Instructions
                // OP    REG      MEM/REG/CONST
                ["mov", reg, parameter] if self.is_valid_register(*reg) => {
                    let (size_option, memory_address) = self.get_pointer_argument_size(parameter);
                    let (constant, assumed_size) = self.parse_value_from_parameter(memory_address, size_option)?;

                    let is_immediate: bool = parse_string_to_usize(parameter).is_some();

                    if let Some(reg_size) = get_register_size(reg) {
                        let assumed_size_v = assumed_size.value();
                        let reg_size_v = reg_size.value();
                        let invalid = if is_immediate {
                            assumed_size_v > reg_size_v
                        } else {
                            assumed_size_v != reg_size_v
                        };
                        if invalid {
                            return Err(ErrorCode::InvalidValue(format!("Source {parameter} of size ({assumed_size_v}) bytes bytes and destination {reg} of size ({reg_size_v}) bytes are not compatible")));
                        }
                    }
                    self.mov_reg_const(reg, constant)?;

                },
                // OP       MEM               REG/CONST
                ["mov", memory_address, parameter] => {

                    if self.memory_manager.is_memory_operand(parameter) {
                        return Err(ErrorCode::InvalidValue(format!("Direct memory transfer is not supported.")));
                    }
                    let (size_option_src, _) = self.get_pointer_argument_size(parameter);

                    let (size_option_dst, sliced_memory_string) = self.get_pointer_argument_size(memory_address);
                    
                    let is_immediate = parse_string_to_usize(parameter).is_some();
                    if let Some(size_dst) = size_option_dst {
                        if let Some(size_src) = size_option_src {
                            let message = format!("Source {parameter} of size ({}) bytes bytes and destination {memory_address} of size ({}) bytes are not compatible", size_src.value(), size_dst.value());
                            let is_invalid = if is_immediate {
                                size_dst.value() < size_src.value()
                            } else {
                                size_dst != size_src
                            };
                            if is_invalid {
                                return Err(ErrorCode::InvalidValue(message));
                            }
                        }
                    };
                    

                    // Calculate effective address of destination            
                    match self.memory_manager.calculate_effective_address(sliced_memory_string, &self.registers, true) {
                        // Destination is valid address
                        Ok(parsed_address) => {
                            // Get value and size of memory to mov into destination address
                            let (constant, _ ) = {
                                let (v, s) = self.parse_value_from_parameter(parameter, size_option_src)?;
                                (v, size_option_src.unwrap_or(s))
                            };
                            let (_, assumed_size_memory) = self.parse_value_from_parameter(sliced_memory_string, size_option_dst)?; 
                            
                            match assumed_size_memory {
                                VariableSize::Byte => self.memory_manager.set_byte(parsed_address, constant as u8)?,
                                VariableSize::Word => self.memory_manager.set_word(parsed_address, constant as u16)?,
                                VariableSize::DoubleWord => self.memory_manager.set_dword(parsed_address, constant as u32)?,
                            };
                        },
                        // Destination is not valid address
                        Err(error) => return Err(error)
                    }
                },  
                // HELP COMMAND
                ["mov", _rest @ ..] => {
                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;
                    println!("{}", Instruction::get_help_string(Instruction::Mov));
                    return Err(ErrorCode::InvalidOpcode(line.join(", ")));
                },
                // ADD/SUB Instructions
                //         OP              REG          MEM/REG/CONST
                [op @ ("add"|"sub"), reg, parameter] if self.is_valid_register(reg) => {
                    let is_addition = *op == "add";
                    let is_immediate = parse_string_to_usize(parameter).is_some();
                                                                // No "WORD PTR" etc.
                    let (size_option, trimmed_parameter) = self.get_pointer_argument_size(parameter);
    
                    let (constant, assumed_size) = self.parse_value_from_parameter(trimmed_parameter, size_option)?;

                    if let Some(reg_size) = get_register_size(reg) {
                        let assumed_size_v = assumed_size.value();
                        let reg_size_v = reg_size.value();
                        let invalid = if is_immediate {
                            assumed_size_v > reg_size_v
                        } else {
                            assumed_size_v != reg_size_v
                        };
                        if invalid {
                            return Err(ErrorCode::InvalidValue(format!("Source {parameter} of size ({assumed_size_v}) bytes bytes and destination {reg} of size ({reg_size_v}) bytes are not compatible")));
                        }
                    }
                    self.add_or_sub_reg_const(reg, constant, is_addition)?;
                },
                //         OP               MEM          REG/CONST
                [op @ ("add"|"sub"), memory_address, parameter] => {
                    let is_addition = *op == "add";
                    let is_immediate: bool = parse_string_to_usize(parameter).is_some();

                    if self.memory_manager.is_memory_operand(parameter) {
                        return Err(ErrorCode::InvalidValue(format!("Direct memory transfer is not supported.")));
                    }
                    
                    let (size_option_src, _) = self.get_pointer_argument_size(parameter);
                    let (size_option_dest, memory_address_str_dest) = self.get_pointer_argument_size(memory_address);

                    if size_option_src.is_some() && size_option_dest.is_some() {
                        let a = size_option_src.unwrap().value();
                        let b = size_option_dest.unwrap().value();

                        let invalid = if is_immediate {
                            a > b
                        } else {
                            a != b
                        };
                        if invalid {
                            return Err(ErrorCode::InvalidValue(format!("Source {parameter} of size ({a}) bytes bytes and destination {memory_address} of size ({b}) bytes are not compatible")));
                        }
                    }


                    // Calculate effective address of destination            
                    match self.memory_manager.calculate_effective_address(memory_address_str_dest, &self.registers, true) {
                        // Destination is valid address
                        Ok(parsed_address) => {
                            // Get value and size of memory to mov into destination address
                            let (constant, assumed_size ) = {
                                self.parse_value_from_parameter(parameter, size_option_src)?
                            };

                            // Save EAX value
                            let eax: u32 = self.get_register_value("EAX")?;
                            // Load constant into EAX
                            self.registers[get_register("EAX")].load_dword(constant);

                            match assumed_size {
                                VariableSize::Byte => self.add_or_sub_mem_reg(parsed_address, "AL", is_addition)?,
                                VariableSize::Word =>  self.add_or_sub_mem_reg(parsed_address, "AX", is_addition)?,
                                VariableSize::DoubleWord =>  self.add_or_sub_mem_reg(parsed_address, "EAX", is_addition)?,
                            };
                            // Load original value back.
                            self.registers[get_register("EAX")].load_dword(eax);


                        },
                        // Destination is not valid address
                        Err(error) => return Err(error)
                    }                    
                },
                ["add", _rest @ ..] => {
                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;
                    println!("{}", Instruction::get_help_string(Instruction::Add));
                    return Err(ErrorCode::InvalidOpcode(line.join(", ")));
                },
                ["sub", _rest @ ..] => {
                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;
                    println!("{}", Instruction::get_help_string(Instruction::Sub));
                    
                    return Err(ErrorCode::InvalidOpcode(line.join(", ")));
                },
                // MULL / IMUL
                [op @ ("mul" | "imul"), parameter] => {
                    // Determine size of the operand
                    let (size_option_src, memory_address_str_src) = self.get_pointer_argument_size(parameter);

                    let (src_value, backup_size) = self.parse_value_from_parameter(memory_address_str_src, None)?;

                    let size = size_option_src.unwrap_or(backup_size);

                    self.mul_value(src_value, size, *op == "imul")?;
                },
                [op @ ("mul" | "imul"), _rest @ ..] => {
                    let signed = *op == "imul";
                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;
                    if signed {
                        println!("{}", Instruction::get_help_string(Instruction::Imul));
                    } else {
                        println!("{}", Instruction::get_help_string(Instruction::Mul));
                    }
                    
                    return Err(ErrorCode::InvalidOpcode(line.join(", ")));
                },
                // DIV / IDIV Instructions
                [op @ ("div" | "idiv"), parameter] => {
                    // Determine size of the operand
                    let (size_option_src, memory_address_str_src) = self.get_pointer_argument_size(parameter);

                    let (src_value, backup_size) = self.parse_value_from_parameter(memory_address_str_src, None)?;

                    let size = size_option_src.unwrap_or(backup_size);

                    self.div_value(src_value, size, *op == "idiv")?;
                },
                [op @ ("div" | "idiv"), _rest @ ..] => {
                    let signed = *op == "idiv";
                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;
                    if signed {
                        println!("{}", Instruction::get_help_string(Instruction::Idiv));
                    } else {
                        println!("{}", Instruction::get_help_string(Instruction::Div));
                    }
                    
                    return Err(ErrorCode::InvalidOpcode(line.join(", ")));
                },                           
                [op @ ("shr" | "shl"), register, parameter] if self.is_valid_register(register)=> {
                    // Determine size of the operand
                    /*          
                    "The 'shr' instruction shifts the bits of the operand to the right.
                    Syntax:
                        shr <reg>, <const>
                        shr [<mem>], <const>
                        shr <reg>, <cl>
                        shr [<mem>], <cl>
                    */
                    let is_immediate: bool = parse_string_to_usize(parameter).is_some();

                    let is_cl = *parameter == "CL";
                    let value_src = if is_cl {
                        self.get_register_value("CL")?
                    } else if is_immediate {
                        parse_string_to_usize(parameter).unwrap()
                    } else {
                        return Err(ErrorCode::InvalidValue(
                            format!("Parameter {parameter} can only be an immediate value, or 'CL'")
                        ));
                    };

                    let register_size_option = get_register_size(register);
                    let register_size = match register_size_option {
                        Some(size) => size,
                        None => return Err(ErrorCode::InvalidRegister(
                            format!("{} is an invalid register.", register) // unreachable anyway
                        )),
                    };
                    let is_shr = *op == "shr";
                    match register_size {
                        VariableSize::Byte => {
                            let value_masked = value_src & 0b111; // Mask to 3 bits (0-7)
                            if value_src > 7 {
                                println!("[WARNING] Shift Amount is truncated to 3 bits: ({}).", value_masked);
                            }
                            let top: bool = register.ends_with("H");
                            let current_value: u8 = self.registers[get_register(register)].get_byte(top);
                            let carry_flag = (current_value >> (value_masked - 1)) & 1; // Last bit shifted out
                            let new_value = if is_shr {
                                current_value >> value_masked
                            } else {
                                current_value << value_masked
                            };
                            self.registers[get_register(register)].load_byte(new_value, top);
                            self.set_flags(new_value as usize, VariableSize::Word, false);
                            self.set_flag(Flag::Carry, carry_flag == 1);
                        },
                        VariableSize::Word => {
                            let value_masked = value_src & 0b11111; // Mask to 5 bits (0-31)
                            if value_src > 31 {
                                println!("[WARNING] Shift Amount is truncated to 5 bits: ({}).", value_masked);
                            }
                            let current_value: u16 = self.registers[get_register(register)].get_word();
                            let carry_flag = (current_value >> (value_masked - 1)) & 1; // Last bit shifted out
                            let new_value = if is_shr {
                                current_value >> value_masked
                            } else {
                                current_value << value_masked
                            };
                            self.registers[get_register(register)].load_word(new_value);
                            self.set_flags(new_value as usize, VariableSize::Word, false);
                            self.set_flag(Flag::Carry, carry_flag == 1);
                        },
                        VariableSize::DoubleWord => {
                            let value_masked = value_src & 0b11111; // Mask to 5 bits (0-31)
                            if value_src > 31 {
                                println!("[WARNING] Shift Amount is truncated to 5 bits: ({}).", value_masked);
                            }
                            let current_value: u32 = self.registers[get_register(register)].get_dword();
                            let carry_flag = (current_value >> (value_masked - 1)) & 1; // Last bit shifted out

                            let new_value = if is_shr {
                                current_value >> value_masked
                            } else {
                                current_value << value_masked
                            };
                            self.registers[get_register(register)].load_dword(new_value);
                            self.set_flags(new_value as usize, VariableSize::Word, false);
                            self.set_flag(Flag::Carry, carry_flag == 1);
                        },
                    }
                },
                [op @ ("shr" | "shl"), memory_address, parameter] => {
                    // Determine size of the operand
                    /*          
                    "The 'shr' instruction shifts the bits of the operand to the right.
                    Syntax:
                        shr <reg>, <const>
                        shr [<mem>], <const>
                        shr <reg>, <cl>
                        shr [<mem>], <cl>
                    */
                    let is_immediate: bool = parse_string_to_usize(parameter).is_some();

                    let is_cl = *parameter == "CL";
                    let value_src = if is_cl {
                        self.get_register_value("CL")?
                    } else if is_immediate {
                        parse_string_to_usize(parameter).unwrap()
                    } else {
                        return Err(ErrorCode::InvalidValue(
                            format!("Parameter {parameter} can only be an immediate value, or 'CL'")
                        ));
                    };

                    let (size_option_dst, trimmed_dst) = self.get_pointer_argument_size(memory_address);
                    let (memory_value, size_dst) = self.parse_value_from_parameter(trimmed_dst, size_option_dst)?;
                        

                    let destination = self.memory_manager.calculate_effective_address(trimmed_dst, &self.registers, true)?;

                    let is_shr = *op == "shr";
                    // println!("{:?} {}", size_dst, memory_address);
                    match size_dst {
                        VariableSize::Byte => {
                            let value_masked = value_src & 0b111; // Mask to 3 bits (0-7)
                            if value_src > 7 {
                                println!("[WARNING] Shift Amount is truncated to 3 bits ({}).", value_masked);
                            }
                            let current_value = memory_value as u8;
                            let carry_flag = (current_value >> (value_masked - 1)) & 1; // Last bit shifted out

                            let new_value = if is_shr {
                                current_value >> value_masked
                            } else {
                                current_value << value_masked
                            };
                            self.set_flags(new_value as usize, VariableSize::Word, false);
                            self.memory_manager.set_byte(destination, new_value)?;
                            self.set_flag(Flag::Carry, carry_flag == 1);
                        },
                        VariableSize::Word => {
                            let value_masked = value_src & 0b11111; // Mask to 5 bits (0-31)
                            if value_src > 31 {
                                println!("[WARNING] Shift Amount is truncated to 5 bits: ({}).", value_masked);
                            }
                            let current_value = memory_value as u16;
                            let carry_flag = (current_value >> (value_masked - 1)) & 1; // Last bit shifted out

                            let new_value = if is_shr {
                                current_value >> value_masked
                            } else {
                                current_value << value_masked
                            };
                            self.memory_manager.set_word(destination, new_value)?;
                            self.set_flags(new_value as usize, VariableSize::Word, false);
                            self.set_flag(Flag::Carry, carry_flag == 1);
                        },
                        VariableSize::DoubleWord => {
                            let value_masked = value_src & 0b11111; // Mask to 5 bits (0-31)
                            if value_src > 31 {
                                println!("[WARNING] Shift Amount is truncated to 5 bits: ({}).", value_masked);
                            }
                            let current_value = memory_value as u32;
                            let carry_flag = (current_value >> (value_masked - 1)) & 1; // Last bit shifted out

                            let new_value = if is_shr {
                                current_value >> value_masked
                            } else {
                                current_value << value_masked
                            };
                            self.memory_manager.set_dword(destination, new_value)?;
                            self.set_flag(Flag::Carry, carry_flag == 1);
                        },
                    }
                },
                // PRINT  Instructions
                ["print", parameter] => { 

                    let args: Vec<&str> = parameter.split_whitespace().collect();
                    let trimmed_parameter = if args[0] == "char" {
                        args[1..].join(" ")
                    } else {
                        args.join(" ")
                    };
                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;
                    if let Some((start_char, end_char)) = parameter.chars().next().zip(parameter.chars().rev().next()) {
                        if start_char == '\'' && end_char == '\'' {
                            println!("[PRINT]@[IP={ip}]:\t{parameter}\n");    
                            continue;          
                        }
                    }


                    let (size, memory_address_str_src) = self.get_pointer_argument_size(&trimmed_parameter);
                    let (src_value, _) = self.parse_value_from_parameter(memory_address_str_src, size)?;
                    if args.into_iter().skip(2).next() == Some("char") {
                        if let Some(src_value_char) = std::char::from_u32(src_value) {
                            println!("[PRINT]@[IP={ip}] {parameter}: {0}\n", src_value_char);
                        }
                    } else {
                        // Check if argument is a string literal and remove surrounding quotes
                        println!("[PRINT]@[IP={ip}] {parameter}: {0}\n", src_value);
                    }            
                },
                ["print", number, memory_address_maybe_ch]  => { //if self.memory_manager.is_memory_operand(memory_address) &&
                                                                                            //parse_string_to_usize(*number).is_some() => {

                    let args: Vec<&str> = memory_address_maybe_ch.split_whitespace().collect();

                    let (ch, memory_address) = match args.as_slice() {
                        ["char", address] => (true, *address),
                        [address] => (false, *address),
                        _ => return Err(ErrorCode::InvalidOpcode(line.join(", ")))
                    };

                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;

                    let (size_option_src, trimmed_address) = self.get_pointer_argument_size(memory_address);
                    let size = size_option_src.unwrap_or(VariableSize::Byte);
                    if let Ok(parsed_address) = self.memory_manager.calculate_effective_address(trimmed_address, &self.registers, true) {
                        let (value, _) = self.parse_value_from_parameter(*number, None)?;
                        self.memory_manager.check_memory_address(parsed_address+(value as usize)*size.value())?;
                        let ip = self.registers[get_register("IP")].get_word();
                        print!("[PRINT]@[IP={ip}][{parsed_address}..{}]:\t[", parsed_address+(value as usize)-1);
                        for i in 0..value {
                            match size {
                                VariableSize::Byte => {
                                    let src_value = self.memory_manager.get_byte(parsed_address+(i as usize)*size.value())?;

                                    if ch {
                                        if let Some(src_value_char) = std::char::from_u32(src_value as u32) {
                                            print!("{0}", src_value_char);
                                        } else {
                                            print!("{}", format!("{: >width$}", src_value, width=4));
                                        }
                                    } else {
                                        print!("{}", format!("{: >width$}", src_value, width=4));
                                    }
                                },
                                VariableSize::Word => {
                                    let src_value = self.memory_manager.get_word(parsed_address+(i as usize)*size.value())?;
                                    if ch {
                                        if let Some(src_value_char) = std::char::from_u32(src_value as u32) {
                                            print!("{0}", src_value_char);
                                        } else {
                                            print!("{}", format!("{: >width$}", src_value, width=4));
                                        }
                                    } else {
                                        print!("{}", format!("{: >width$}", src_value, width=4));
                                    }                                    
                                },
                                VariableSize::DoubleWord => {
                                    let src_value = self.memory_manager.get_dword(parsed_address+(i as usize)*size.value())?;
                                    if ch {
                                        if let Some(src_value_char) = std::char::from_u32(src_value) {
                                            print!("{0}", src_value_char);
                                        } else {
                                            print!("{0} ", src_value);
                                        }
                                    } else {
                                        print!("{0} ", src_value);
                                    }  
                                },  
                            }
                        }
                        println!("]");

                    // } else {
                    //     return Err(ErrorCode::InvalidValue(format!("Integer {number} could not be parsed.")))
                    // }
                    } else {
                        return Err(ErrorCode::InvalidValue(format!("Memory address {memory_address} could not be parsed.")))
                    }
                },
                ["NOP"] => {},
                [variable_name, define_as @ ("db"|"dw"|"dd"), rest @ ..] if self.memory_manager.is_valid_variable_name(*variable_name) => {
                // (parse_string_to_usize(*data).is_some() || self.memory_manager.is_valid_array(*data).is_ok() )=> {
                    let size: VariableSize = match *define_as {
                        "db" => VariableSize::Byte,
                        "dw" => VariableSize::Word,
                        "dd" => VariableSize::DoubleWord,
                        _ => return Err(ErrorCode::InvalidValue("Invalid Variable Size".to_string()))
                    };
                    let mut bytes: Vec<u32> = Vec::new();
                    for (_, &arg) in rest.iter().enumerate() {
                        // Check if argument is a string literal and remove surrounding quotes
                        if let Some((start_char, end_char)) = arg.chars().next().zip(arg.chars().rev().next()) {
                            if (start_char == '"' && end_char == '"') || (start_char == '\'' && end_char == '\'') {
                                let inner = &arg[1..arg.len() - 1];
                                for c in inner.chars() {
                                    bytes.push(c as u32);
                                }
                            } else {
                                // Handle other cases (numeric values, etc.)
                                if let Some(value) = parse_string_to_usize(arg) {
                                    bytes.push(value as u32);
                                } else {
                                    return Err(ErrorCode::InvalidValue(format!("Could not parse {arg}")));
                                }
                            }
                        } else {
                            // Handle other cases (numeric values, etc.)
                            if let Some(value) = parse_string_to_usize(arg) {
                                bytes.push(value as u32);                   
                            } else {
                                return Err(ErrorCode::InvalidValue(format!("Could not parse {arg}")));
                            }
                        }
                    }
                    let _ = skip_lines(lines_to_skip);
                    lines_to_skip += 1;
                    if let Err(error) = self.memory_manager.save_variable(variable_name.to_string(), &bytes, size) {
                        return Err(error);
                    }
                },
                //////// JUMPS ////////////
                ["jmp", label] => {
                    if let Err(error) = self.jump_to(label) {     
                        let _ = skip_lines(lines_to_skip);
                        lines_to_skip += 1;
                        println!("{}", Instruction::get_help_string(Instruction::Jmp));
                        return Err(error);
                    }
                },
                // JUMPS
                [_flag @ ("je"|"jz"|"jne"|"jnz"), label] => {

                    let equal = *_flag == "je" || *_flag == "jz";
                    if equal != self.is_flag_on(Flag::Zero) {
                        continue;
                    }
                    if let Err(error) = self.jump_to(label) {  
                        let _ = skip_lines(lines_to_skip);
                        lines_to_skip += 1;   
                        match *_flag {
                            "je" =>  println!("{}", Instruction::get_help_string(Instruction::Je)),
                            "jz" =>  println!("{}", Instruction::get_help_string(Instruction::Jz)),
                            "jne" =>  println!("{}", Instruction::get_help_string(Instruction::Jne)),
                            "jnz" =>  println!("{}", Instruction::get_help_string(Instruction::Jnz)),
                            _ => {}
                        }          
                        return Err(error);
                    }
                },     
                [_flag @ ("jg" | "jge"), label] => {
                    let is_jg = *_flag == "jg";

                    if is_jg && self.is_flag_on(Flag::Zero) {
                        continue;
                    }
                    if self.is_flag_on(Flag::Sign) != self.is_flag_on(Flag::Overflow) {
                        continue;
                    }
            
                    if let Err(error) = self.jump_to(label) {   
                        let _ = skip_lines(lines_to_skip);
                        lines_to_skip += 1;
                        match *_flag {
                            "jg" =>  println!("{}", Instruction::get_help_string(Instruction::Jg)),
                            "jge" =>  println!("{}", Instruction::get_help_string(Instruction::Jge)),
                            _ => {}
                        }                
                        return Err(error);
                    }
                },
                [_flag @ ("jl" | "jle"), label] => {
                    let is_jl = *_flag == "jl";

                    if is_jl && self.is_flag_on(Flag::Zero) {
                        continue;
                    }

                    if self.is_flag_on(Flag::Sign) == self.is_flag_on(Flag::Overflow) {
                        continue;
                    }
            
                    if let Err(error) = self.jump_to(label) {   
                        let _ = skip_lines(lines_to_skip);
                        lines_to_skip += 1; 
                        match *_flag {
                            "jl" =>  println!("{}", Instruction::get_help_string(Instruction::Jl)),
                            "jle" =>  println!("{}", Instruction::get_help_string(Instruction::Jle)),
                            _ => {}
                        }   
                        return Err(error);
                    }
                },
                [_flag @ ("ja" | "jae"), label] => {
                    let is_ja = *_flag == "ja";

                    if self.is_flag_on(Flag::Carry) ||(is_ja && self.is_flag_on(Flag::Zero)) {
                        continue;
                    }
            
                    if let Err(error) = self.jump_to(label) {     
                        let _ = skip_lines(lines_to_skip);
                        lines_to_skip += 1;  
                        match *_flag {
                            "ja" =>  println!("{}", Instruction::get_help_string(Instruction::Ja)),
                            "jae" =>  println!("{}", Instruction::get_help_string(Instruction::Jae)),
                            _ => {}
                        }           
                        return Err(error);
                    }
                },
                [_flag @ ("jb" | "jbe"), label] => {
                    let is_jb = *_flag == "jb";

                    if self.is_flag_on(Flag::Carry) || (is_jb && self.is_flag_on(Flag::Zero)) {
                        continue;
                    }

                    if let Err(error) = self.jump_to(label) {       
                        let _ = skip_lines(lines_to_skip);
                        lines_to_skip += 1;   
                        match *_flag {
                            "jb" =>  println!("{}", Instruction::get_help_string(Instruction::Jb)),
                            "jbe" =>  println!("{}", Instruction::get_help_string(Instruction::Jbe)),
                            _ => {}
                        }        
                        return Err(error);
                    }
                },
                // CMP
                ["cmp", first_operand, second_operand] => {

                    let (first_size_option, trimmed_first_parameter) = self.get_pointer_argument_size(first_operand);
                    let (first_operand_value, first_operand_size) = self.parse_value_from_parameter(trimmed_first_parameter, first_size_option)?;


                    let (second_size_option, trimmed_second_parameter) = self.get_pointer_argument_size(second_operand);
                    let (second_operand_value, second_operand_size) = self.parse_value_from_parameter(trimmed_second_parameter, second_size_option)?;

                    let second_is_immediate: bool = parse_string_to_usize(second_operand).is_some();

                    if second_is_immediate && first_operand_size.value() < second_operand_size.value(){
                        return Err(ErrorCode::InvalidValue(
                                format!("Target memory pointer size ({}) bytes doesn't match second parameter size ({}) bytes",
                                                first_operand_size.value(),          second_operand_size.value())
                            ))

                    } else if !second_is_immediate && first_operand_size != second_operand_size {
                        return Err(ErrorCode::InvalidValue(
                                        format!("Target memory pointer size ({}) bytes doesn't match second parameter size ({}) bytes",
                                                        first_operand_size.value(),          second_operand_size.value())
                                    ))
                    }
                    // Initialize variables to store parsed values
                    let first_value = first_operand_value as isize;
                    let second_value = second_operand_value as isize;

                    let result = first_value - second_value;
                    // Set flags based on comparison results
                    self.set_flag(Flag::Zero, result == 0);
                    self.set_flag(Flag::Carry, first_value < second_value);
                    self.set_flag(Flag::Overflow, (first_value < 0 && second_value > 0 && result > 0) || (first_value > 0 && second_value < 0 && result < 0));
                    self.set_flag(Flag::Sign, result < 0);
                    self.set_flag(Flag::Parity, result.count_ones() % 2 == 0);
                },
                ["call", label] => {
                    let ip: u32 = self.get_register_value("IP")?;
                    match self.jump_to(label) {
                        Ok(_) => {
                            self.memory_manager.push_to_stack(ip, 
                                                              VariableSize::Word, 
                                                              &mut self.registers[get_register("ESI")])?;
                        },
                        Err(error) => return Err(error),
                    }
                },
                ["ret"] => {
                    let ip_from_stack = self.memory_manager.pop_from_stack(VariableSize::Word, 
                                                                                &mut self.registers[get_register("ESI")])?;
                    // I would use JUMP_TO but it hates me apparently.
                    self.registers[get_register("IP")].load_word(ip_from_stack as u16);
                    // self.lines.set_ip(ip_from_stack as usize);	
                } 
                // STACK OPERATIONS
                ["push", parameter] => {
                    // No "WORD PTR" etc.

                    let (size_option, trimmed_parameter) = self.get_pointer_argument_size(parameter);

                    let (value, size) = self.parse_value_from_parameter(trimmed_parameter, size_option)?;
                    self.memory_manager.push_to_stack(value, size, &mut self.registers[get_register("SI")])?;

                },
                ["pop", parameter] => {
                    // No "WORD PTR" etc.
                    let (size_option, trimmed_parameter) = self.get_pointer_argument_size(parameter);

                    let (_, size) = self.parse_value_from_parameter(trimmed_parameter, size_option)?;
                    let popped_value = self.memory_manager.pop_from_stack(size, &mut self.registers[get_register("SI")])?;
                    if self.is_valid_register(parameter) {
                        match size {
                            VariableSize::Byte => return Err(ErrorCode::InvalidValue("POP can only receive a 16-bit or 32-bit parameter.".to_string())),
                            VariableSize::Word => self.registers[get_register(parameter)].load_word(popped_value as u16),
                            VariableSize::DoubleWord => self.registers[get_register(parameter)].load_dword(popped_value),
                        }
                    } else {
                        // Calculate effective address of destination            
                        match self.memory_manager.calculate_effective_address(trimmed_parameter, &self.registers, true) {
                            // Destination is valid address
                            Ok(parsed_address) => {
                                match size {
                                    VariableSize::Byte =>  return Err(ErrorCode::InvalidValue("POP can only receive a 16-bit or 32-bit parameter.".to_string())),
                                    VariableSize::Word => self.memory_manager.set_word(parsed_address, popped_value as u16)?,
                                    VariableSize::DoubleWord => self.memory_manager.set_dword(parsed_address, popped_value)?,
                                }
                            },
                            Err(error) => return Err(error)
                        }
                    }

                },
                // IGNORE LABELS
                [label] if label.ends_with(":") => {
                    let no_colon = &label[..label.len()-1];
                    // Found no label
                    if self.memory_manager.labels.get(no_colon).is_none(){
                        let error_msg = format!("Unknown instruction: {:?}.\nPerhaps you misspelt the label name?", label);
                        return Err(ErrorCode::InvalidOpcode(error_msg));
                    }
                },
                // SKIP PROCS
                [proc, arg @ ("PROC"|"END")] => {
                    let end = *arg == "END";

                    let end_ip = if let Some((_, end_ip)) = self.memory_manager.procs.get(*proc) {
                        *end_ip
                    } else {
                        let error_msg: String = format!("Unknown instruction: {:?}.\nPerhaps you misspelt the proc name?", proc);
                        return Err(ErrorCode::InvalidOpcode(error_msg));
                    };

                    if end {
                        let error_msg: String = format!("Must return in proc.");
                        return Err(ErrorCode::InvalidOpcode(error_msg));
                    } else {
                        let caller_ip = format!("{}", end_ip);
                        if let Err(error) = self.jump_to(&&caller_ip.as_str()) {
                            return Err(error);
                        }
                    }
                },
                
                // NO MATCH
                _ => {
                    let error_msg = format!("Unknown instruction: {:?}", line);
                    return Err(ErrorCode::InvalidOpcode(error_msg));
                    // Handle unrecognized instructions
                }
            }
            // Won't panic
            // If some JUMP was made, this will update the lines module.
            // If no JUMP was made, this basically does nothing, as it sets lines' ip to itself.
            self.lines.set_ip(self.get_register_value("IP")? as usize);
        }
        for _ in 0..lines_to_skip {
            println!("");
        }
        Ok(())
    }

    fn jump_to(&mut self, label: &&str) -> Result<usize, ErrorCode> {
        if let Some(address) = self.memory_manager.labels.get(*label) {
            // self.lines.set_ip(*address);
            self.registers[get_register("IP")].load_word(*address as u16);
            Ok(*address)
        } else if let Some((start_ip, _)) = self.memory_manager.procs.get(*label) {
            // self.lines.set_ip(*start_ip);
            self.registers[get_register("IP")].load_word(*start_ip as u16);
            Ok(*start_ip)
        } else {
            let (size_option, trimmed) = self.get_pointer_argument_size(label);

            if trimmed != *label {
                return Err(ErrorCode::InvalidPointer(
                    format!("Invalid Syntax.")
                ));
            }
            
            let target =  if self.memory_manager.is_memory_operand(label) {
                self.memory_manager.calculate_effective_address(label, &self.registers, true)?
                
            } else {
                let (a, _) = self.parse_value_from_parameter(label, size_option)?;
                a as usize
            };

            if target > MEMORY_SIZE - 1 {
                return Err(ErrorCode::InvalidPointer(
                    format!("Target IP \"{}\" is outside of memory bounds.", target)
                ));
            }

            self.registers[get_register("IP")].load_word(target as u16);
            // self.lines.set_ip(target);
            Ok(target)
        }

    }

    fn is_flag_on(&self, flag: Flag) -> bool {
        // No need to check, because we know FLAG is a valid register.
        // Also, it's 16 bits, i.e., we can convert it without issues.
        let value = self.get_register_value("FLAG").unwrap() as u16;
        value & flag.value() != 0
    }

    // MOV operations
    // REG <- Reg, Const, Var, Mem
    // Mem <- Reg, Const, Var, Mem
    // Var <- Reg, Const, Var, Mem
    // Const <- NOTHING, Const can't be moved into

    fn mov_reg_const(&mut self, dest: &str, constant: u32) -> Result<(), ErrorCode>{
        let size = match dest {
            reg if reg.ends_with('L')|reg.ends_with('H') => {
                self.registers[get_register(dest)].load_byte(constant as u8, reg.ends_with('H'));
                VariableSize::Byte
            },
            "AX"|"BX"|"CX"|"DX"|"SI"|"DI"|"FLAG"|"IP" => {
                self.registers[get_register(dest)].load_word(constant as u16);
                VariableSize::Word
            },
            reg if reg.starts_with("E") => {
                self.registers[get_register(dest)].load_dword(constant);
                VariableSize::DoubleWord
            }
            _ => return Err(ErrorCode::InvalidRegister(dest.to_string()))
        };
        self.set_flags(constant as usize, size, false);
        Ok(())
    }

    fn add_or_sub_mem_reg(&mut self, memory_address: usize, src: &str, is_addition: bool) -> Result<(), ErrorCode> {

        let (result, overflowed, size) = match src {
            reg if reg.ends_with('L') || reg.ends_with('H') => {
                let src_value = self.registers[get_register(src)].get_byte(reg.ends_with('H'));
                let dest_value = self.memory_manager.get_byte(memory_address)?;

                let (sum, overflowed) = if is_addition {
                    dest_value.overflowing_add(src_value)
                } else {
                    dest_value.overflowing_sub(src_value)
                };
                self.memory_manager.set_byte(memory_address, sum)?;

                (sum as u32, overflowed, VariableSize::Byte)
            },
            "AX" | "BX" | "CX" | "DX" | "SI" | "DI" | "FLAG" | "IP" | "SP" => {
                let src_value = self.registers[get_register(src)].get_word();
                let dest_value = self.memory_manager.get_word(memory_address)?;

                let (sum, overflowed) = if is_addition {
                    dest_value.overflowing_add(src_value)
                } else {
                    dest_value.overflowing_sub(src_value)
                };

                self.memory_manager.set_word(memory_address, sum)?;

                (sum as u32, overflowed, VariableSize::Word)
            },
            reg if reg.starts_with('E') => {
                self.memory_manager.check_memory_address(memory_address)?;

                let src_value = self.registers[get_register(src)].get_dword();
                let dest_value = self.memory_manager.get_dword(memory_address)?;

                let (sum, overflowed) = if is_addition {
                    dest_value.overflowing_add(src_value)
                } else {
                    dest_value.overflowing_sub(src_value)
                };

                self.memory_manager.set_dword(memory_address, sum)?;

                (sum as u32, overflowed, VariableSize::DoubleWord)
            },
            _ => return Err(ErrorCode::InvalidRegister(src.to_string())),
        };
        self.set_flags(result as usize, size, overflowed);
        Ok(())
    }
    
    fn add_or_sub_reg_const(&mut self, dest: &str, constant: u32, is_addition: bool) -> Result<(), ErrorCode>{
        match dest {
            "AX"|"BX"|"CX"|"DX"|"ESI"|"EDI"|"FLAG"|"IP"|"SP" => {
                if constant > u16::MAX as u32 {
                    return Err(ErrorCode::InvalidValue(format!("Value {constant} can't fit in {dest}")));
                }
                let dest_value = self.get_register_value(dest)? as u16;
                let (sum, overflowed) = if is_addition {
                    dest_value.overflowing_add(constant as u16)
                } else {
                    dest_value.overflowing_sub(constant as u16)
                };                
                self.registers[get_register(dest)].load_word(sum);
                self.set_flags(sum as usize,VariableSize::Byte,  overflowed);
            }
            reg if reg.ends_with('L') || reg.ends_with('H') => {
                if constant > u8::MAX as u32 {
                    return Err(ErrorCode::InvalidValue(format!("Value {constant} can't fit in {dest}")));
                }
                let dest_value = self.get_register_value(dest)? as u8;
                let (sum, overflowed) = if is_addition {
                    dest_value.overflowing_add(constant as u8)
                } else {
                    dest_value.overflowing_sub(constant as u8)
                };                
                self.registers[get_register(dest)].load_byte(sum,  reg.ends_with('H'));
                self.set_flags(sum as usize,VariableSize::Byte,  overflowed);
            },
            reg if reg.starts_with('E') => {
                let dest_value = self.get_register_value(dest)? as u32;

                let (sum, overflowed) = if is_addition {
                    dest_value.overflowing_add(constant)
                } else {
                    dest_value.overflowing_sub(constant)
                };          
                self.registers[get_register(dest)].load_dword(sum);
                
                self.set_flags(sum as usize,VariableSize::DoubleWord, overflowed);
            },
            _ => {
                return Err(ErrorCode::InvalidRegister(dest.to_string()));
            }
        };
        Ok(())
    }

    fn set_register_value(&mut self, register: &str, value: u32) -> Result<(), ErrorCode>{
        match get_register_size(register) {
            Some(VariableSize::Byte) => {
                if value > u8::MAX as u32 {
                    return Err(ErrorCode::InvalidValue(format!("Value {value} can't fit in {register}")));
                }
                self.registers[get_register(register)].load_byte(value.try_into().unwrap(), register.ends_with('H'));
                Ok(())
            },
            Some(VariableSize::Word) => {
                if value > u16::MAX as u32 {
                    return Err(ErrorCode::InvalidValue(format!("Value {value} can't fit in {register}")));
                }
                self.registers[get_register(register)].load_word(value.try_into().unwrap());
                Ok(())
            }
            Some(VariableSize::DoubleWord) => {
                self.registers[get_register(register)].load_dword(value.try_into().unwrap());
                Ok(())
            }
            _ => Err(ErrorCode::InvalidRegister(register.to_string()))
        }
    }

    fn set_flag(&mut self, flag: Flag, value: bool) {
        if value {
            let current = self.registers[get_register("FLAG")].get_word();
            self.registers[get_register("FLAG")].load_word(current | flag.value());
        } else {
            let current = self.registers[get_register("FLAG")].get_word();
            self.registers[get_register("FLAG")].load_word(current & !flag.value());
        }
    }

    //////////// MUL ////////////
    // Multiply the value in the source register by the value in AX register.

    fn mul_value(&mut self, src_value: u32, size: VariableSize , signed: bool) -> Result<(), ErrorCode>{
        match size {
            VariableSize::Byte => self.mul_8bit(src_value as u8, signed),
            VariableSize::Word => self.mul_16bit(src_value as u16, signed),
            VariableSize::DoubleWord => self.mul_32bit(src_value, signed),
        }
    }
    
    // Function to multiply 8-bit values and store the result in AX register.
    fn mul_8bit(&mut self, src_value: u8, signed: bool) -> Result<(), ErrorCode>{
        let ax_value = self.get_register_value("AX")?;
        let al_value = ax_value as u8;
    
        let result = if signed {
            let al_signed = al_value as i8;
            let src_signed = src_value as i8;
            (al_signed as i16 * src_signed as i16) as u16
        } else {
            (al_value as u16 * src_value as u16) as u16
        };
    
        let overflow_condition = result > 0xFF;
    
        self.set_flag(Flag::Carry, overflow_condition);
        self.set_flag(Flag::Overflow, overflow_condition);
        self.set_register_value("AX", result as u32)?;
        Ok(())
    }
    
    // Function to multiply 16-bit values and store the result in DX:AX register.
    fn mul_16bit(&mut self, src_value: u16, signed: bool) -> Result<(), ErrorCode> {
        let ax_value = self.get_register_value("AX")?;
    
        let result = if signed {
            let ax_signed = ax_value as i16;
            let src_signed = src_value as i16;
            (ax_signed as i32 * src_signed as i32) as u32
        } else {
            (ax_value as u32 * src_value as u32) as u32
        };
    
        let overflow_condition = result > 0xFFFF;
    
        self.set_flag(Flag::Carry, overflow_condition);
        self.set_flag(Flag::Overflow, overflow_condition);
        self.set_register_value("AX", result as u32)?;
        self.set_register_value("DX", (result >> 16) as u32)?;
        Ok(())
    }
    
    // Function to multiply 32-bit values and store the result in EDX:EAX register.
    fn mul_32bit(&mut self, src_value: u32, signed: bool)-> Result<(), ErrorCode> {
        let eax_value = self.get_register_value("EAX")?;
    
        let result = if signed {
            let eax_signed = eax_value as i32;
            let src_signed = src_value as i32;
            (eax_signed as i64 * src_signed as i64) as u64
        } else {
            (eax_value as u64 * src_value as u64) as u64
        };
    
        let overflow_condition = result > 0xFFFFFFFF;
    
        self.set_flag(Flag::Carry, overflow_condition);
        self.set_flag(Flag::Overflow, overflow_condition);
        self.set_register_value("EAX", result as u32)?;
        self.set_register_value("EDX", (result >> 32) as u32)?;
        Ok(())
    }



    /////////// DIV ////////////
    // Divide the value in the source register by the value in the AX register.
    fn div_value(&mut self, src_value: u32, size: VariableSize , signed: bool) -> Result<(), ErrorCode>{
        match size {
            VariableSize::Byte => self.div_8bit(src_value as u8, signed),
            VariableSize::Word => self.div_16bit(src_value as u16, signed),
            VariableSize::DoubleWord => self.div_32bit(src_value, signed),
        }
    }
    // AX / src_value, AH = AX % src_value
    fn div_8bit(&mut self, src_value: u8, signed: bool) -> Result<(), ErrorCode> {
        let ax_value = self.get_register_value("AX")? as u16;
        if src_value == 0 {
            return Err(ErrorCode::DivisionByZero);
        }

        let (quotient, remainder) = if signed {
            let al_signed = (ax_value as u8) as i8;
            let src_signed = src_value as i8;
            let (q, r) = (al_signed / src_signed, al_signed % src_signed);
            (q as u8, r as u8)
        } else {
            ((ax_value / src_value as u16) as u8, (ax_value % src_value as u16) as u8)
        };

        self.set_register_value("AL", quotient as u32)?;
        self.set_register_value("AH", remainder as u32)?;
        Ok(())
    }

    // DX:AX / src_value, DX = DX:AX % src_value
    fn div_16bit(&mut self, src_value: u16, signed: bool) -> Result<(), ErrorCode> {
        let ax_value = self.get_register_value("AX")?;
        let dx_value = self.get_register_value("DX")?;
        let dividend = ((dx_value as u32) << 16) | (ax_value as u32);
        if src_value == 0 {
            return Err(ErrorCode::DivisionByZero);
        }
        let (quotient, remainder) = if signed {
            let dividend_signed = dividend as i32;
            let src_signed = src_value as i16;
            let (q, r) = (dividend_signed / src_signed as i32, dividend_signed % src_signed as i32);
            (q as u16, r as u16)
        } else {
            ((dividend / src_value as u32) as u16, (dividend % src_value as u32) as u16)
        };

        self.set_register_value("AX", quotient as u32)?;
        self.set_register_value("DX", remainder as u32)?;
        Ok(())
    }

    fn div_32bit(&mut self, src_value: u32, signed: bool) -> Result<(), ErrorCode> {
        // Fetch EAX and EDX register values
        let eax_value = self.get_register_value("EAX")?;  // Assuming this retrieves u32
        let edx_value = self.get_register_value("EDX")?;  // Assuming this retrieves u32

        // Create the 64-bit dividend from EDX:EAX
        let dividend = ((edx_value as u64) << 32) | (eax_value as u64);

        // Handle division by zero
        if src_value == 0 {
            return Err(ErrorCode::DivisionByZero);
        }

        // Perform division and modulus based on signed or unsigned mode
        let ((quotient, remainder), overflowed) = if signed {
            // Signed division and modulus
            let dividend_signed = dividend as i64;
            let src_signed = src_value as i32;
            
            // Check for overflow in division (when dividing i64::MIN by -1)
            if src_signed == -1 && dividend_signed == i64::MIN {
                return Err(ErrorCode::Overflow);
            }

            let (q, overflow1) = dividend_signed.overflowing_div(src_signed as i64);
            let (r, overflow2) = dividend_signed.overflowing_rem(src_signed as i64);
            ((q as u32, r as u32), overflow1 || overflow2)
        } else {
            // Unsigned division and modulus
            let (q, overflow1) = dividend.overflowing_div(src_value as u64);
            let (r, overflow2) = dividend.overflowing_rem(src_value as u64);
            ((q as u32, r as u32), overflow1 || overflow2)
        };

        // Check for overflow and return error if necessary
        if overflowed {
            return Err(ErrorCode::Overflow);
        }

        // Update EAX and EDX registers with quotient and remainder
        self.set_register_value("EAX", quotient)?;
        self.set_register_value("EDX", remainder)?;
        Ok(())
    }
}


impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let length = self.registers.clone().into_iter().len();
        for i in 0..length {
            let register: &Register = &self.registers[i];
            let low_byte = (register.get_word() & 0xFF) as u8;
            let high_byte = ((register.get_word() >> 8) & 0xFF) as u8;
            let reg_string = format!("Register {}:\t{}\t({:08b} {:08b})", register.name, register.get_word(), high_byte, low_byte);
            write!(f, "{reg_string}  ")?;
            
            let start_index = if i > 6 {
                MEMORY_SIZE - ((length-i) * 24)
            } else {
                i * 24
            };

            let this_line = self.memory_manager._get_memory(start_index, 24);
            write!(f, "{: <6}:", start_index)?;
            for j in 0..6 {
                write!(f, "{:02X} ", this_line[j*4])?;
                write!(f, "{:02X} ", this_line[j*4+1])?;
                write!(f, "{:02X} ", this_line[j*4+2])?;
                write!(f, "{:02X}  ", this_line[j*4+3], )?;
            }
            writeln!(f, "")?;
        }
        Ok(())
    }
}
