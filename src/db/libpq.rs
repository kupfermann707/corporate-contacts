use crate::db::wire::QueryResult;
use std::ffi::{CStr, CString};
use std::sync::Arc;

pub struct LibPqLoader {
    _handle: *mut libc::c_void,
    pq_connectdb: unsafe extern "C" fn(*const libc::c_char) -> *mut libc::c_void,
    pq_status: unsafe extern "C" fn(*mut libc::c_void) -> libc::c_int,
    pq_error_message: unsafe extern "C" fn(*mut libc::c_void) -> *const libc::c_char,
    pq_finish: unsafe extern "C" fn(*mut libc::c_void),
    pq_exec: unsafe extern "C" fn(*mut libc::c_void, *const libc::c_char) -> *mut libc::c_void,
    pq_result_status: unsafe extern "C" fn(*mut libc::c_void) -> libc::c_int,
    pq_result_error_message: unsafe extern "C" fn(*mut libc::c_void) -> *const libc::c_char,
    pq_ntuples: unsafe extern "C" fn(*mut libc::c_void) -> libc::c_int,
    pq_nfields: unsafe extern "C" fn(*mut libc::c_void) -> libc::c_int,
    pq_fname: unsafe extern "C" fn(*mut libc::c_void, libc::c_int) -> *const libc::c_char,
    pq_getvalue: unsafe extern "C" fn(*mut libc::c_void, libc::c_int, libc::c_int) -> *const libc::c_char,
    pq_getisnull: unsafe extern "C" fn(*mut libc::c_void, libc::c_int, libc::c_int) -> libc::c_int,
    pq_clear: unsafe extern "C" fn(*mut libc::c_void),
    pq_cmd_tuples: unsafe extern "C" fn(*mut libc::c_void) -> *const libc::c_char,
}

unsafe impl Send for LibPqLoader {}
unsafe impl Sync for LibPqLoader {}

impl LibPqLoader {
    pub fn try_load() -> Result<Arc<Self>, String> {
        let names = ["libpq.so.5", "libpq.so.5.18", "libpq.so"];
        let mut handle = std::ptr::null_mut();

        for name in &names {
            let c_name = CString::new(*name).map_err(|e| e.to_string())?;
            handle = unsafe { libc::dlopen(c_name.as_ptr(), libc::RTLD_NOW) };
            if !handle.is_null() {
                break;
            }
        }

        if handle.is_null() {
            return Err("Библиотека libpq.so.5 не найдена в системе".to_string());
        }

        unsafe {
            macro_rules! load_sym {
                ($sym:expr) => {{
                    let s = CString::new($sym).unwrap();
                    let ptr = libc::dlsym(handle, s.as_ptr());
                    if ptr.is_null() {
                        libc::dlclose(handle);
                        return Err(format!("Не найден символ {} в libpq", $sym));
                    }
                    std::mem::transmute(ptr)
                }};
            }

            Ok(Arc::new(Self {
                _handle: handle,
                pq_connectdb: load_sym!("PQconnectdb"),
                pq_status: load_sym!("PQstatus"),
                pq_error_message: load_sym!("PQerrorMessage"),
                pq_finish: load_sym!("PQfinish"),
                pq_exec: load_sym!("PQexec"),
                pq_result_status: load_sym!("PQresultStatus"),
                pq_result_error_message: load_sym!("PQresultErrorMessage"),
                pq_ntuples: load_sym!("PQntuples"),
                pq_nfields: load_sym!("PQnfields"),
                pq_fname: load_sym!("PQfname"),
                pq_getvalue: load_sym!("PQgetvalue"),
                pq_getisnull: load_sym!("PQgetisnull"),
                pq_clear: load_sym!("PQclear"),
                pq_cmd_tuples: load_sym!("PQcmdTuples"),
            }))
        }
    }
}

pub struct LibPqClient {
    conn: *mut libc::c_void,
    loader: Arc<LibPqLoader>,
}

unsafe impl Send for LibPqClient {}

impl Drop for LibPqClient {
    fn drop(&mut self) {
        if !self.conn.is_null() {
            unsafe {
                (self.loader.pq_finish)(self.conn);
            }
            self.conn = std::ptr::null_mut();
        }
    }
}

impl LibPqClient {
    pub fn connect(loader: Arc<LibPqLoader>, conninfo: &str) -> Result<Self, String> {
        let c_conninfo = CString::new(conninfo).map_err(|e| e.to_string())?;
        let conn = unsafe { (loader.pq_connectdb)(c_conninfo.as_ptr()) };
        if conn.is_null() {
            return Err("Не удалось инициализировать подключение к PostgreSQL".to_string());
        }

        let status = unsafe { (loader.pq_status)(conn) };
        if status != 0 {
            // CONNECTION_OK = 0
            let err_ptr = unsafe { (loader.pq_error_message)(conn) };
            let err_msg = if !err_ptr.is_null() {
                unsafe { CStr::from_ptr(err_ptr).to_string_lossy().into_owned() }
            } else {
                "Неизвестная ошибка подключения".to_string()
            };
            unsafe { (loader.pq_finish)(conn) };
            return Err(format!("Ошибка подключения libpq: {}", err_msg.trim()));
        }

        Ok(Self { conn, loader })
    }

    pub fn execute(&mut self, query: &str) -> Result<QueryResult, String> {
        let c_query = CString::new(query).map_err(|e| e.to_string())?;
        let res = unsafe { (self.loader.pq_exec)(self.conn, c_query.as_ptr()) };
        if res.is_null() {
            let err_ptr = unsafe { (self.loader.pq_error_message)(self.conn) };
            let err_msg = if !err_ptr.is_null() {
                unsafe { CStr::from_ptr(err_ptr).to_string_lossy().into_owned() }
            } else {
                "Null результат запроса".to_string()
            };
            return Err(format!("Ошибка libpq: {}", err_msg.trim()));
        }

        let status = unsafe { (self.loader.pq_result_status)(res) };
        // 1 = PGRES_COMMAND_OK, 2 = PGRES_TUPLES_OK
        if status != 1 && status != 2 {
            let err_ptr = unsafe { (self.loader.pq_result_error_message)(res) };
            let err_msg = if !err_ptr.is_null() {
                unsafe { CStr::from_ptr(err_ptr).to_string_lossy().into_owned() }
            } else {
                format!("Код ошибки запроса: {}", status)
            };
            unsafe { (self.loader.pq_clear)(res) };
            return Err(format!("Ошибка выполнения SQL: {}", err_msg.trim()));
        }

        let n_tuples = unsafe { (self.loader.pq_ntuples)(res) } as usize;
        let n_fields = unsafe { (self.loader.pq_nfields)(res) } as usize;

        let mut columns = Vec::with_capacity(n_fields);
        for col in 0..n_fields {
            let name_ptr = unsafe { (self.loader.pq_fname)(res, col as libc::c_int) };
            let name = if !name_ptr.is_null() {
                unsafe { CStr::from_ptr(name_ptr).to_string_lossy().into_owned() }
            } else {
                format!("col_{}", col)
            };
            columns.push(name);
        }

        let mut rows = Vec::with_capacity(n_tuples);
        for r in 0..n_tuples {
            let mut row = Vec::with_capacity(n_fields);
            for c in 0..n_fields {
                let is_null = unsafe { (self.loader.pq_getisnull)(res, r as libc::c_int, c as libc::c_int) };
                if is_null == 1 {
                    row.push(String::new());
                } else {
                    let val_ptr = unsafe { (self.loader.pq_getvalue)(res, r as libc::c_int, c as libc::c_int) };
                    let val = if !val_ptr.is_null() {
                        unsafe { CStr::from_ptr(val_ptr).to_string_lossy().into_owned() }
                    } else {
                        String::new()
                    };
                    row.push(val);
                }
            }
            rows.push(row);
        }

        let affected = {
            let cmd_ptr = unsafe { (self.loader.pq_cmd_tuples)(res) };
            if !cmd_ptr.is_null() {
                let s = unsafe { CStr::from_ptr(cmd_ptr).to_string_lossy() };
                s.trim().parse::<u64>().unwrap_or(0)
            } else {
                0
            }
        };

        unsafe {
            (self.loader.pq_clear)(res);
        }

        Ok(QueryResult {
            columns,
            rows,
            affected_rows: affected,
        })
    }
}
